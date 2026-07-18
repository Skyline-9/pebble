#![forbid(unsafe_code)]

//! Packaged polyglot extraction integration tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::ingestion::{
    DiagnosticKind, EdgeKind, Extractor, FileExtraction, Language, MAX_CHUNK_BYTES, StructuralEdge,
};
use pebble_core::repository::{RepositoryConfig, RepositorySnapshot, SourceFile, SystemGit};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const FIXTURES: [(&str, Language, &str); 14] = [
    (
        "sample.c",
        Language::C,
        "#include <stdio.h>\nint greet(char *name) {\n  return puts(name);\n}\n",
    ),
    (
        "sample.cpp",
        Language::Cpp,
        "#include <string>\nvoid greet(std::string name) {\n  consume(name);\n}\n",
    ),
    (
        "sample.cs",
        Language::CSharp,
        "using System;\nclass Greeter {\n  void Greet(string name) { Console.WriteLine(name); }\n}\n",
    ),
    (
        "sample.go",
        Language::Go,
        "package sample\nimport \"fmt\"\nfunc greet(name string) {\n fmt.Println(name)\n}\n",
    ),
    (
        "sample.java",
        Language::Java,
        "import java.util.Objects;\nclass Greeter {\n void greet(String name) { Objects.requireNonNull(name); }\n}\n",
    ),
    (
        "sample.js",
        Language::JavaScript,
        "import { log } from \"./log.js\";\nfunction greet(name) {\n log(name);\n}\n",
    ),
    (
        "sample.jsx",
        Language::Jsx,
        "import React from \"react\";\nfunction Greeting({name}) {\n return <h1>{formatName(name)}</h1>;\n}\n",
    ),
    (
        "sample.kt",
        Language::Kotlin,
        "import kotlin.io.println\nfun greet(name: String) {\n println(name)\n}\n",
    ),
    (
        "sample.py",
        Language::Python,
        "import os\ndef greet(name):\n    print(name)\n",
    ),
    (
        "sample.rb",
        Language::Ruby,
        "require 'json'\ndef greet(name)\n  puts(name)\nend\n",
    ),
    (
        "sample.rs",
        Language::Rust,
        "use std::fmt;\nfn greet(name: &str) {\n println!(\"{name}\");\n}\n",
    ),
    (
        "sample.swift",
        Language::Swift,
        "import Foundation\nfunc greet(name: String) {\n print(name)\n}\n",
    ),
    (
        "sample.ts",
        Language::TypeScript,
        "import { log } from \"./log\";\nfunction greet(name: string): void {\n log(name);\n}\n",
    ),
    (
        "sample.tsx",
        Language::Tsx,
        "import React from \"react\";\nfunction Greeting({name}: {name: string}) {\n return <h1>{formatName(name)}</h1>;\n}\n",
    ),
];

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pebble-polyglot-{}-{suffix}", std::process::id()));
        fs::create_dir_all(path.join(".pebble"))?;
        run_git(&path, &["init", "-q"])?;
        run_git(
            &path,
            &[
                "-c",
                "user.name=Pebble",
                "-c",
                "user.email=pebble@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        )?;
        fs::write(
            path.join(".pebble/pebble.toml"),
            concat!(
                "schema = 1\n",
                "repository_id = \"polyglot.repo\"\n",
                "include = [\"**/*\"]\n",
                "exclude = []\n\n",
                "[language_overrides]\n",
                "\"override.inc\" = \"rust\"\n",
            ),
        )?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn sources(&self) -> Result<(RepositoryConfig, Vec<SourceFile>), Box<dyn std::error::Error>> {
        let config = RepositoryConfig::load(self.path())?;
        let mut snapshot = RepositorySnapshot::open(self.path(), &config, &SystemGit::discover()?)?;
        let sources = snapshot.by_ref().collect::<Result<Vec<_>, _>>()?;
        Ok((config, sources))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_git(repository: &Path, arguments: &[&str]) -> std::io::Result<()> {
    let status = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repository)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("test Git command failed"))
    }
}

fn by_path(sources: Vec<SourceFile>) -> BTreeMap<String, SourceFile> {
    sources
        .into_iter()
        .map(|source| (source.path().to_owned(), source))
        .collect()
}

#[test]
fn every_language_mode_extracts_real_source() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    for (path, _, source) in FIXTURES {
        fs::write(fixture.path().join(path), source)?;
    }
    fs::write(fixture.path().join("override.inc"), "fn overridden() {}\n")?;
    let (config, sources) = fixture.sources()?;
    let extractor = Extractor::new(&config);
    let sources = by_path(sources);

    for (path, language, _) in FIXTURES {
        let extraction = extractor.extract(&sources[path]);
        assert_eq!(extraction.language(), Some(language), "{path}");
        assert!(!extraction.chunks().is_empty(), "{path}");
        assert!(!extraction.symbols().is_empty(), "{path}");
        assert!(extraction.diagnostics().is_empty(), "{path}");
        assert_eq!(extraction, extractor.extract(&sources[path]), "{path}");
    }
    assert_eq!(
        extractor.extract(&sources["override.inc"]).language(),
        Some(Language::Rust)
    );
    Ok(())
}

#[test]
fn structural_assertions_are_conservative_and_visible() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    for (path, _, source) in FIXTURES {
        fs::write(fixture.path().join(path), source)?;
    }
    let (config, sources) = fixture.sources()?;
    let extractor = Extractor::new(&config);
    let sources = by_path(sources);
    for (path, _, _) in FIXTURES {
        let extraction = extractor.extract(&sources[path]);
        for kind in [
            EdgeKind::Defines,
            EdgeKind::Imports,
            EdgeKind::Calls,
            EdgeKind::References,
        ] {
            assert!(
                extraction.edges().iter().any(|edge| edge.kind() == kind),
                "{path} missing {kind:?}"
            );
        }
        assert!(FileExtraction::symbols(&extraction).iter().all(|symbol| {
            symbol.start_line() > 0
                && symbol.end_line() >= symbol.start_line()
                && !symbol.name().is_empty()
        }));
    }
    Ok(())
}

#[test]
fn unknown_and_malformed_sources_fall_back_with_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    fs::write(
        fixture.path().join("notes.txt"),
        "First paragraph.\n\nSecond paragraph.\n",
    )?;
    fs::write(
        fixture.path().join("broken.rs"),
        "fn broken( {\nstill text\n",
    )?;
    let (config, sources) = fixture.sources()?;
    let extractor = Extractor::new(&config);
    let sources = by_path(sources);

    let unknown = extractor.extract(&sources["notes.txt"]);
    assert_eq!(unknown.language(), None);
    assert!(!unknown.chunks().is_empty());
    assert!(unknown.symbols().is_empty());
    assert_eq!(
        unknown.diagnostics()[0].kind(),
        DiagnosticKind::UnknownLanguage
    );

    let malformed = extractor.extract(&sources["broken.rs"]);
    assert_eq!(malformed.language(), Some(Language::Rust));
    assert!(!malformed.chunks().is_empty());
    assert!(malformed.symbols().is_empty());
    assert_eq!(
        malformed.diagnostics()[0].kind(),
        DiagnosticKind::ParseError
    );
    Ok(())
}

#[test]
fn chunks_have_stable_ids_exact_lines_and_a_hard_byte_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let long = format!("heading\n\n{}\nlast\n", "λ".repeat(MAX_CHUNK_BYTES));
    fs::write(fixture.path().join("large.unknown"), long)?;
    let (config, sources) = fixture.sources()?;
    let extractor = Extractor::new(&config);
    let sources = by_path(sources);

    let first = extractor.extract(&sources["large.unknown"]);
    let second = extractor.extract(&sources["large.unknown"]);
    assert_eq!(first.chunks(), second.chunks());
    assert!(first.chunks().len() >= 3);
    assert!(first.chunks().iter().all(|chunk| {
        chunk.text().len() <= MAX_CHUNK_BYTES
            && chunk.start_line() > 0
            && chunk.end_line() >= chunk.start_line()
    }));
    assert_eq!(first.chunks()[0].start_line(), 1);
    assert_eq!(first.chunks()[0].end_line(), 1);
    Ok(())
}

#[test]
fn repeated_bounded_slices_have_unique_stable_chunk_ids() -> Result<(), Box<dyn std::error::Error>>
{
    let repeated = "x".repeat(MAX_CHUNK_BYTES * 2);
    let first = extract_source("repeated.unknown", &repeated)?;
    let second = extract_source("repeated.unknown", &repeated)?;
    let first_ids = first
        .chunks()
        .iter()
        .map(|chunk| chunk.id().as_str())
        .collect::<Vec<_>>();
    let second_ids = second
        .chunks()
        .iter()
        .map(|chunk| chunk.id().as_str())
        .collect::<Vec<_>>();

    assert_eq!(first_ids, second_ids);
    assert_eq!(first_ids.len(), 2);
    assert_ne!(first_ids[0], first_ids[1]);
    assert_eq!(first.chunks()[0].start_line(), 1);
    assert_eq!(first.chunks()[1].start_line(), 1);
    assert_eq!(first.chunks()[0].text(), first.chunks()[1].text());
    Ok(())
}

#[test]
fn same_name_same_line_declarations_have_unique_stable_symbol_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "function same() {} function same(value: string) { return value; }\n";
    let first = extract_source("overloads.ts", source)?;
    let second = extract_source("overloads.ts", source)?;
    let first_ids = first
        .symbols()
        .iter()
        .map(|symbol| symbol.id().as_str())
        .collect::<Vec<_>>();
    let second_ids = second
        .symbols()
        .iter()
        .map(|symbol| symbol.id().as_str())
        .collect::<Vec<_>>();

    assert_eq!(first_ids, second_ids);
    assert_eq!(first_ids.len(), 2);
    assert_ne!(first_ids[0], first_ids[1]);
    assert!(first.symbols().iter().all(|symbol| symbol.name() == "same"));
    assert!(
        first
            .symbols()
            .iter()
            .all(|symbol| symbol.start_line() == 1)
    );
    Ok(())
}

#[test]
fn javascript_edges_name_modules_and_called_members() -> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract_source(
        "edges.js",
        concat!(
            "import { send } from \"./transport.js\";\n",
            "const legacy = require(\"./legacy.js\");\n",
            "client.send();\n",
        ),
    )?;
    let imports = targets(&extraction, EdgeKind::Imports);
    let calls = targets(&extraction, EdgeKind::Calls);

    assert_eq!(imports, vec!["./legacy.js", "./transport.js"]);
    assert!(calls.contains(&"send"));
    assert!(!calls.contains(&"client"));
    assert!(!imports.iter().any(|target| target.contains("import")));
    assert!(!imports.iter().any(|target| target.contains("require")));
    Ok(())
}

#[test]
fn fieldless_call_nodes_target_callees_not_arguments() -> Result<(), Box<dyn std::error::Error>> {
    let kotlin = extract_source("callee.kt", "fun greet(name: String) { println(name) }\n")?;
    let swift = extract_source("callee.swift", "func greet(name: String) { print(name) }\n")?;

    assert_eq!(targets(&kotlin, EdgeKind::Calls), vec!["println"]);
    assert_eq!(targets(&swift, EdgeKind::Calls), vec!["print"]);
    Ok(())
}

#[test]
fn go_packages_are_not_imports_and_declaration_names_are_not_references()
-> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract_source(
        "declarations.go",
        concat!(
            "package sample\n",
            "import \"fmt\"\n",
            "func greet(name string) {\n",
            " local := name\n",
            " fmt.Println(local)\n",
            "}\n",
        ),
    )?;
    let imports = targets(&extraction, EdgeKind::Imports);
    let references = targets(&extraction, EdgeKind::References);

    assert_eq!(imports, vec!["fmt"]);
    assert!(!references.contains(&"sample"));
    assert!(!references.contains(&"greet"));
    assert_eq!(
        references
            .iter()
            .filter(|target| **target == "name")
            .count(),
        1
    );
    assert_eq!(
        references
            .iter()
            .filter(|target| **target == "local")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn javascript_parameter_local_and_function_names_are_not_references()
-> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract_source(
        "declarations.js",
        "function greet(name) { const local = name; return local; }\n",
    )?;
    let references = targets(&extraction, EdgeKind::References);

    assert_eq!(references, vec!["local", "name"]);
    assert!(!references.contains(&"greet"));
    Ok(())
}

fn extract_source(path: &str, source: &str) -> Result<FileExtraction, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    fs::write(fixture.path().join(path), source)?;
    let (config, sources) = fixture.sources()?;
    let extractor = Extractor::new(&config);
    let sources = by_path(sources);
    Ok(extractor.extract(&sources[path]))
}

fn targets(extraction: &FileExtraction, kind: EdgeKind) -> Vec<&str> {
    let mut targets = extraction
        .edges()
        .iter()
        .filter(|edge| edge.kind() == kind)
        .map(StructuralEdge::target)
        .collect::<Vec<_>>();
    targets.sort_unstable();
    targets
}
