//! Thin CLI dispatch over the model-free core service.

mod plan2;

use std::io::{self, Write};

use pebble_core::domain::{Citation, RepositoryId, WorktreeRevision};
use pebble_core::retrieval::SearchRequest;
use pebble_core::service::{IndexResult, PebbleService, ServiceError};
use serde::Serialize;

use crate::arguments::{Arguments, Operation, ReadArguments, SearchArguments};

/// Dispatch one parsed invocation and return its documented process code.
pub fn dispatch(
    service: &PebbleService,
    arguments: Arguments,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    match execute(service, arguments.command, arguments.json, stdout) {
        Ok(code) => Ok(code),
        Err(error) => {
            writeln!(stderr, "pebble: {error}")?;
            Ok(if error.is_evidence_failure() { 1 } else { 2 })
        }
    }
}

fn execute(
    service: &PebbleService,
    operation: Operation,
    json: bool,
    stdout: &mut impl Write,
) -> Result<u8, ServiceError> {
    match operation {
        Operation::Init { repository } => {
            let result = service.initialize(&repository)?;
            output(stdout, json, &result, || {
                format!(
                    "initialized {} at {}",
                    result.repository_id,
                    result.repository.display()
                )
            })?;
        }
        Operation::Register {
            repository,
            alternate_worktree,
        } => {
            let result = service.register(&repository, alternate_worktree)?;
            output(stdout, json, &result, || {
                format!(
                    "registered {} at {}",
                    result.repository_id,
                    result.repository.display()
                )
            })?;
        }
        Operation::Index { repository } => {
            let result = service.index(&repository)?;
            write_index(stdout, json, &result)?;
        }
        Operation::Rebuild { repository } => {
            let result = service.rebuild(&repository)?;
            write_index(stdout, json, &result)?;
        }
        Operation::Watch { repository, once } => {
            if once {
                let result = service.watch_once(&repository)?;
                write_index(stdout, json, &result)?;
            } else {
                watch(service, &repository, json, stdout)?;
            }
        }
        Operation::Search(arguments) => search(service, arguments, json, stdout)?,
        Operation::Read(arguments) => {
            let result = service.read(citation(arguments)?)?;
            output(stdout, json, &result, || result.content.clone())?;
        }
        Operation::Health { repository } => {
            let repository = repository_id(&repository)?;
            let result = service.health(&repository)?;
            output(stdout, json, &result, || {
                if result.healthy {
                    format!(
                        "{} is healthy at generation {}",
                        result.repository_id,
                        result.generation.as_deref().unwrap_or("unknown")
                    )
                } else {
                    format!(
                        "{} is unhealthy: {}",
                        result.repository_id,
                        result.issue.as_deref().unwrap_or("unknown issue")
                    )
                }
            })?;
            return Ok(u8::from(!result.healthy));
        }
        Operation::Traces { repository, limit } => {
            let repository = repository_id(&repository)?;
            let traces = service.traces(&repository, limit)?;
            output(stdout, json, &traces, || {
                format!("{} retrieval trace(s)", traces.len())
            })?;
        }
        Operation::Serve => {
            return Err(ServiceError::Configuration(
                "stdio MCP adapter is not available in this build".to_owned(),
            ));
        }
        operation => return plan2::dispatch_plan2(service, operation, json, stdout),
    }
    Ok(0)
}

fn search(
    service: &PebbleService,
    arguments: SearchArguments,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let repository = repository_id(&arguments.repository)?;
    let response = service.search(&repository, search_request(arguments)?)?;
    if json {
        return write_json(stdout, response.packet());
    }
    writeln!(
        stdout,
        "{} evidence item(s), approximately {} tokens",
        response.packet().items().len(),
        response.estimated_tokens()
    )
    .map_err(operational)?;
    for item in response.packet().items() {
        writeln!(
            stdout,
            "{}@{}:{}:{}-{}\n{}",
            item.citation.repository(),
            item.citation.revision(),
            item.citation.path(),
            item.citation.start_line(),
            item.citation.end_line(),
            item.content
        )
        .map_err(operational)?;
    }
    Ok(())
}

fn watch(
    service: &PebbleService,
    repository: &std::path::Path,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let watcher = service.watch(repository)?;
    loop {
        if let Some(job) = watcher
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(operational)?
        {
            if json {
                write_json(
                    stdout,
                    &WatchOutput {
                        generation: job.generation().to_string(),
                        full_scan: job.full_scan(),
                    },
                )?;
            } else {
                writeln!(stdout, "indexed generation {}", job.generation()).map_err(operational)?;
            }
        }
    }
}

fn search_request(arguments: SearchArguments) -> Result<SearchRequest, ServiceError> {
    let mut request = SearchRequest::new(arguments.query).map_err(usage)?;
    request = request
        .with_repository(arguments.repository)
        .map_err(usage)?
        .with_budget_tokens(arguments.budget)
        .map_err(usage)?
        .with_max_results(arguments.limit)
        .map_err(usage)?;
    if let Some(revision) = arguments.revision {
        request = request.with_revision(revision).map_err(usage)?;
    }
    if let Some(path) = arguments.path {
        request = request.with_path_prefix(path).map_err(usage)?;
    }
    if let Some(language) = arguments.language {
        request = request.with_language(language).map_err(usage)?;
    }
    if !arguments.kinds.is_empty() {
        request = request.with_kinds(arguments.kinds).map_err(usage)?;
    }
    Ok(request)
}

fn citation(arguments: ReadArguments) -> Result<Citation, ServiceError> {
    let repository = repository_id(&arguments.repository)?;
    let revision = revision(&arguments.revision)?;
    Citation::new(
        repository,
        revision,
        arguments.path,
        arguments.start_line,
        arguments.end_line,
    )
    .map_err(usage)
}

fn repository_id(value: &str) -> Result<RepositoryId, ServiceError> {
    RepositoryId::try_from(value.to_owned()).map_err(usage)
}

fn revision(value: &str) -> Result<WorktreeRevision, ServiceError> {
    value
        .split_once("+dirty.")
        .map_or_else(
            || WorktreeRevision::clean(value),
            |(base, dirty)| WorktreeRevision::dirty(base, dirty),
        )
        .map_err(usage)
}

fn write_index(
    stdout: &mut impl Write,
    json: bool,
    result: &IndexResult,
) -> Result<(), ServiceError> {
    output(stdout, json, result, || {
        format!(
            "indexed {} generation {} at {}",
            result.repository_id, result.generation, result.revision
        )
    })
}

fn output<T: Serialize>(
    stdout: &mut impl Write,
    json: bool,
    value: &T,
    human: impl FnOnce() -> String,
) -> Result<(), ServiceError> {
    if json {
        write_json(stdout, value)
    } else {
        writeln!(stdout, "{}", human()).map_err(operational)
    }
}

fn write_json(value: &mut impl Write, output: &impl Serialize) -> Result<(), ServiceError> {
    serde_json::to_writer(&mut *value, output).map_err(operational)?;
    writeln!(value).map_err(operational)
}

#[derive(Serialize)]
struct WatchOutput {
    generation: String,
    full_scan: bool,
}

fn usage(error: impl std::fmt::Display) -> ServiceError {
    ServiceError::Usage(error.to_string())
}

fn operational(error: impl std::fmt::Display) -> ServiceError {
    ServiceError::Operational(error.to_string())
}
