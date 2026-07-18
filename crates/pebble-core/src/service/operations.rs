//! Model-free service operations and stable result payloads.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{Citation, RepositoryId};
use crate::index::{GenerationReader, RepositoryCompiler};
use crate::repository::{
    RegisteredRepository, RepositoryConfig, RepositoryRegistry, RepositorySnapshot, SystemGit,
};
use crate::retrieval::{QueryTrace, RetrievalEngine, SearchRequest, SearchResponse};
use crate::watcher::WatchService;

use super::health::{
    canonical_repository, ensure_indexed_citation, exact_lines, map_index_operational,
    map_index_unavailable, map_repository, map_repository_config, map_retrieval, map_watch,
    read_traces,
};
use super::{
    HealthCounts, IndexResult, InitResult, PebbleService, ReadResult, RegisterResult, ServiceError,
    citation_race,
};

impl PebbleService {
    /// Initialize portable repository configuration and private local state.
    ///
    /// # Errors
    ///
    /// Returns a classified configuration or local operational error.
    pub fn initialize(&self, repository: &Path) -> Result<InitResult, ServiceError> {
        let repository = canonical_repository(repository)?;
        let git = SystemGit::discover().map_err(map_repository)?;
        let config =
            RepositoryConfig::initialize(&repository, &git).map_err(map_repository_config)?;
        self.prepare_repository_state(config.repository_id())?;
        Ok(InitResult {
            repository_id: config.repository_id().clone(),
            repository,
            state_root: self.state_root().to_owned(),
        })
    }

    /// Register an initialized checkout in deterministic local state.
    ///
    /// # Errors
    ///
    /// Returns a classified configuration or local operational error.
    pub fn register(
        &self,
        repository: &Path,
        alternate_worktree: bool,
    ) -> Result<RegisterResult, ServiceError> {
        let repository = canonical_repository(repository)?;
        let config = RepositoryConfig::load(&repository).map_err(map_repository_config)?;
        RepositoryRegistry::new(self.state_root())
            .register(config.repository_id(), &repository, alternate_worktree)
            .map_err(map_repository_config)?;
        self.prepare_repository_state(config.repository_id())?;
        Ok(RegisterResult {
            repository_id: config.repository_id().clone(),
            repository,
            alternate_worktree,
        })
    }

    /// Compile and atomically activate the registered checkout.
    ///
    /// # Errors
    ///
    /// Returns a classified configuration or compilation error.
    pub fn index(&self, repository: &Path) -> Result<IndexResult, ServiceError> {
        let (repository, config) = self.registered_repository(repository)?;
        self.compile(&repository, &config)
    }

    /// Build a fresh disposable projection without deleting any existing path.
    ///
    /// # Errors
    ///
    /// Returns a classified configuration or compilation error.
    pub fn rebuild(&self, repository: &Path) -> Result<IndexResult, ServiceError> {
        self.index(repository)
    }

    /// Start the bounded native watcher for a registered checkout.
    ///
    /// # Errors
    ///
    /// Returns a classified configuration, path, or native watcher error.
    pub fn watch(&self, repository: &Path) -> Result<WatchService, ServiceError> {
        let (repository, config) = self.registered_repository(repository)?;
        let generations = self.prepare_repository_state(config.repository_id())?;
        WatchService::start(&repository, &generations, config).map_err(map_watch)
    }

    /// Reconcile once through the actual watcher and return the new generation.
    ///
    /// # Errors
    ///
    /// Returns a classified watcher or compilation error.
    pub fn watch_once(&self, repository: &Path) -> Result<IndexResult, ServiceError> {
        let canonical = canonical_repository(repository)?;
        let config = RepositoryConfig::load(&canonical).map_err(map_repository_config)?;
        let mut watcher = self.watch(&canonical)?;
        watcher.request_reconciliation().map_err(map_watch)?;
        let job = watcher
            .recv_timeout(Duration::from_mins(2))
            .map_err(map_watch)?
            .ok_or_else(|| ServiceError::operational("watch reconciliation timed out"))?;
        watcher.shutdown().map_err(map_watch)?;
        let reader =
            GenerationReader::open_current(&self.layout.generations(config.repository_id()))
                .map_err(map_index_unavailable)?;
        if reader.id() != job.generation() {
            return Err(ServiceError::operational(
                "watcher result does not match active generation",
            ));
        }
        IndexResult::from_reader(config.repository_id().clone(), &reader)
    }

    /// Search one pinned immutable generation and append its bounded trace.
    ///
    /// # Errors
    ///
    /// Returns a classified request, unavailable-index, or trace error.
    pub fn search(
        &self,
        repository: &RepositoryId,
        request: SearchRequest,
    ) -> Result<SearchResponse, ServiceError> {
        self.registration(repository)?;
        let reader = GenerationReader::open_current(&self.layout.generations(repository))
            .map_err(map_index_unavailable)?;
        let request = request
            .with_trace_path(&self.repository_root(repository).join("traces.jsonl"))
            .map_err(map_retrieval)?;
        RetrievalEngine::new(&reader)
            .search(request)
            .map_err(map_retrieval)
    }

    /// Resolve an exact citation only while its indexed worktree still exists.
    ///
    /// # Errors
    ///
    /// Returns stale evidence unless the exact indexed source still resolves.
    pub fn read(&self, citation: Citation) -> Result<ReadResult, ServiceError> {
        let reader =
            GenerationReader::open_current(&self.layout.generations(citation.repository()))
                .map_err(map_index_unavailable)?;
        ensure_indexed_citation(&reader, &citation)?;
        let git = SystemGit::discover().map_err(map_repository)?;
        let registrations = RepositoryRegistry::new(self.state_root())
            .registrations()
            .map_err(map_repository_config)?;
        let mut saw_revision = false;
        let registration = registrations
            .iter()
            .filter(|entry| entry.repository_id() == citation.repository())
            .find(|entry| {
                git.revision(entry.checkout()).is_ok_and(|revision| {
                    saw_revision = true;
                    &revision == citation.revision()
                })
            })
            .ok_or_else(|| {
                if saw_revision {
                    ServiceError::stale("worktree revision no longer matches")
                } else {
                    ServiceError::unavailable("registered checkout is unavailable")
                }
            })?;
        let config =
            RepositoryConfig::load(registration.checkout()).map_err(map_repository_config)?;
        citation_race::run(
            citation_race::RacePoint::BeforeSnapshotOpen,
            registration.checkout(),
        );
        let mut snapshot = RepositorySnapshot::open(registration.checkout(), &config, &git)
            .map_err(map_repository)?;
        if snapshot.revision() != citation.revision() {
            return Err(ServiceError::stale(
                "snapshot revision does not match citation",
            ));
        }
        citation_race::run(
            citation_race::RacePoint::AfterSnapshotOpen,
            registration.checkout(),
        );
        let mut source = None;
        for item in snapshot.by_ref() {
            let item = item.map_err(|error| match error {
                crate::repository::RepositoryError::WorktreeChanged => {
                    ServiceError::stale("worktree changed while reading citation")
                }
                error => map_repository(error),
            })?;
            if item.path() == citation.path() {
                source = Some(item.contents().to_owned());
            }
        }
        let source = source.ok_or_else(|| ServiceError::stale("cited path is unavailable"))?;
        let content = exact_lines(&source, citation.start_line(), citation.end_line())
            .ok_or_else(|| ServiceError::stale("cited line range is unavailable"))?;
        Ok(ReadResult { citation, content })
    }

    /// Read the newest bounded local retrieval traces.
    ///
    /// # Errors
    ///
    /// Returns a classified request, repository, or trace-file error.
    pub fn traces(
        &self,
        repository: &RepositoryId,
        limit: usize,
    ) -> Result<Vec<QueryTrace>, ServiceError> {
        if !(1..=1_000).contains(&limit) {
            return Err(ServiceError::usage(
                "trace limit must be between 1 and 1000",
            ));
        }
        self.registration(repository)?;
        read_traces(
            &self.repository_root(repository).join("traces.jsonl"),
            limit,
        )
    }

    fn compile(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
    ) -> Result<IndexResult, ServiceError> {
        let generations = self.prepare_repository_state(config.repository_id())?;
        let reader = RepositoryCompiler::new(&generations)
            .compile_fresh(repository, config)
            .map_err(map_index_operational)?;
        IndexResult::from_reader(config.repository_id().clone(), &reader)
    }

    fn registered_repository(
        &self,
        repository: &Path,
    ) -> Result<(PathBuf, RepositoryConfig), ServiceError> {
        let repository = canonical_repository(repository)?;
        let config = RepositoryConfig::load(&repository).map_err(map_repository_config)?;
        let registered = RepositoryRegistry::new(self.state_root())
            .registrations()
            .map_err(map_repository_config)?
            .into_iter()
            .any(|entry| {
                entry.repository_id() == config.repository_id() && entry.checkout() == repository
            });
        if !registered {
            return Err(ServiceError::configuration(
                "checkout is not registered for this repository identity",
            ));
        }
        Ok((repository, config))
    }

    pub(super) fn registration(
        &self,
        repository: &RepositoryId,
    ) -> Result<RegisteredRepository, ServiceError> {
        RepositoryRegistry::new(self.state_root())
            .registrations()
            .map_err(map_repository_config)?
            .into_iter()
            .find(|entry| entry.repository_id() == repository)
            .ok_or_else(|| ServiceError::unavailable("repository is not registered"))
    }
}

impl IndexResult {
    fn from_reader(
        repository_id: RepositoryId,
        reader: &GenerationReader,
    ) -> Result<Self, ServiceError> {
        let revision = reader
            .graph()
            .metadata("revision")
            .map_err(map_index_operational)?
            .ok_or_else(|| ServiceError::operational("index revision metadata is missing"))?;
        let counts = reader
            .graph()
            .counts()
            .map(HealthCounts::from)
            .map_err(map_index_operational)?;
        Ok(Self {
            repository_id,
            generation: reader.id().to_string(),
            revision,
            counts,
        })
    }
}
