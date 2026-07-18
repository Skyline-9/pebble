//! Packaged language detection and grammar selection.

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use tree_sitter::Language as Grammar;

use crate::repository::RepositoryConfig;

/// One packaged source-language parsing mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    /// C.
    C,
    /// C++.
    Cpp,
    /// C#.
    CSharp,
    /// Go.
    Go,
    /// Java.
    Java,
    /// JavaScript without JSX.
    JavaScript,
    /// JavaScript with JSX.
    Jsx,
    /// Kotlin.
    Kotlin,
    /// Python.
    Python,
    /// Ruby.
    Ruby,
    /// Rust.
    Rust,
    /// Swift.
    Swift,
    /// TypeScript without JSX.
    TypeScript,
    /// TypeScript with JSX.
    Tsx,
}

pub(super) struct Detector {
    overrides: Vec<(Gitignore, Option<Language>)>,
}

impl Detector {
    pub(super) fn new(config: &RepositoryConfig) -> Self {
        let overrides = config
            .language_overrides()
            .iter()
            .filter_map(|(pattern, name)| {
                let mut builder = GitignoreBuilder::new("");
                builder.add_line(None, pattern).ok()?;
                Some((builder.build().ok()?, Language::from_name(name)))
            })
            .collect();
        Self { overrides }
    }

    pub(super) fn detect(&self, path: &str) -> Option<Language> {
        for (matcher, language) in self.overrides.iter().rev() {
            if matcher
                .matched_path_or_any_parents(Path::new(path), false)
                .is_ignore()
            {
                return *language;
            }
        }
        Language::from_extension(Path::new(path).extension()?.to_str()?)
    }
}

impl Language {
    fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "c" => Some(Self::C),
            "c++" | "cpp" => Some(Self::Cpp),
            "c#" | "csharp" => Some(Self::CSharp),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "javascript" | "js" => Some(Self::JavaScript),
            "jsx" | "javascriptreact" => Some(Self::Jsx),
            "kotlin" => Some(Self::Kotlin),
            "python" => Some(Self::Python),
            "ruby" => Some(Self::Ruby),
            "rust" => Some(Self::Rust),
            "swift" => Some(Self::Swift),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" | "typescriptreact" => Some(Self::Tsx),
            _ => None,
        }
    }

    fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Some(Self::Cpp),
            "cs" => Some(Self::CSharp),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "jsx" => Some(Self::Jsx),
            "kt" | "kts" => Some(Self::Kotlin),
            "py" | "pyi" => Some(Self::Python),
            "rb" => Some(Self::Ruby),
            "rs" => Some(Self::Rust),
            "swift" => Some(Self::Swift),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    pub(super) fn grammar(self) -> Grammar {
        match self {
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::JavaScript | Self::Jsx => tree_sitter_javascript::LANGUAGE.into(),
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    /// Return the stable lowercase language metadata name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Go => "go",
            Self::Java => "java",
            Self::JavaScript => "javascript",
            Self::Jsx => "jsx",
            Self::Kotlin => "kotlin",
            Self::Python => "python",
            Self::Ruby => "ruby",
            Self::Rust => "rust",
            Self::Swift => "swift",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
        }
    }
}
