//! Portable repository configuration.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;

use crate::domain::RepositoryId;

use super::config_fs;
use super::identity::identity_for_remote;
use super::{RepositoryError, SystemGit};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Schema-one portable settings stored in `.pebble/pebble.toml`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepositoryConfig {
    schema: u32,
    repository_id: RepositoryId,
    include: Vec<String>,
    exclude: Vec<String>,
    language_overrides: BTreeMap<String, String>,
}

impl RepositoryConfig {
    /// Load and validate the canonical configuration beneath `repository`.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, oversized files, unknown or duplicate
    /// keys, unsupported schemas, or unsafe path patterns.
    pub fn load(repository: &Path) -> Result<Self, RepositoryError> {
        let bytes = config_fs::read(repository, MAX_CONFIG_BYTES)?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(invalid("file exceeds 1 MiB"));
        }
        let contents = String::from_utf8(bytes).map_err(|_| invalid("file is not valid UTF-8"))?;
        let config: Self =
            toml::from_str(&contents).map_err(|error| invalid(&error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Create a canonical configuration, or load it when already initialized.
    ///
    /// Identity is derived from the normalized `origin` remote when available,
    /// otherwise from a new ULID.
    ///
    /// # Errors
    ///
    /// Returns an error when Git discovery, validation, or durable file
    /// creation fails.
    pub fn initialize(repository: &Path, git: &SystemGit) -> Result<Self, RepositoryError> {
        if config_fs::exists(repository)? {
            return Self::load(repository);
        }
        let config = Self {
            schema: 1,
            repository_id: identity_for_remote(git.origin_remote(repository)?.as_deref())?,
            include: vec!["**/*".to_owned()],
            exclude: Vec::new(),
            language_overrides: BTreeMap::new(),
        };
        config.validate()?;
        let encoded = config.encode();
        if config_fs::create(repository, encoded.as_bytes())? {
            Ok(config)
        } else {
            Self::load(repository)
        }
    }

    /// Return the configuration schema version.
    #[must_use]
    pub const fn schema(&self) -> u32 {
        self.schema
    }

    /// Return the persisted canonical repository identity.
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Return repository-relative include patterns.
    #[must_use]
    pub fn include(&self) -> &[String] {
        &self.include
    }

    /// Return repository-relative exclude patterns.
    #[must_use]
    pub fn exclude(&self) -> &[String] {
        &self.exclude
    }

    /// Return path-pattern to language-name overrides.
    #[must_use]
    pub const fn language_overrides(&self) -> &BTreeMap<String, String> {
        &self.language_overrides
    }

    fn validate(&self) -> Result<(), RepositoryError> {
        if self.schema != 1 {
            return Err(invalid("schema must equal 1"));
        }
        for pattern in self
            .include
            .iter()
            .chain(&self.exclude)
            .chain(self.language_overrides.keys())
        {
            validate_pattern(pattern)?;
        }
        if self.language_overrides.values().any(|language| {
            language.is_empty() || !language.is_ascii() || language.chars().any(char::is_control)
        }) {
            return Err(invalid("language overrides must be nonempty ASCII"));
        }
        Ok(())
    }

    fn encode(&self) -> String {
        let array = |values: &[String]| {
            values
                .iter()
                .map(|value| format!("\"{}\"", escape(value)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut output = format!(
            "schema = 1\nrepository_id = \"{}\"\ninclude = [{}]\nexclude = [{}]\n",
            self.repository_id,
            array(&self.include),
            array(&self.exclude)
        );
        output.push_str("\n[language_overrides]\n");
        for (pattern, language) in &self.language_overrides {
            output.push('"');
            output.push_str(&escape(pattern));
            output.push_str("\" = \"");
            output.push_str(&escape(language));
            output.push_str("\"\n");
        }
        output
    }
}

fn validate_pattern(pattern: &str) -> Result<(), RepositoryError> {
    let path = Path::new(pattern);
    if pattern.is_empty()
        || pattern.contains('\\')
        || pattern.chars().any(char::is_control)
        || path.is_absolute()
        || windows_absolute(pattern)
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid("patterns must be normalized relative slash paths"));
    }
    Ok(())
}

fn windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1..3] == *b":/"
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn invalid(message: &str) -> RepositoryError {
    RepositoryError::InvalidConfig(message.to_owned())
}
