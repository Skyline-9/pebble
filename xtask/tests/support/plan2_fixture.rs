use std::fs;
use std::io;
use std::path::Path;

const INVENTORY: &str = include_str!("../../../config/plan2-dependencies.txt");
const RECORD: &str = "# Dependency decision\n\n## Requirement\n\n## Build in house\n\n\
    ## Candidates\n\n## Measurements\n\n## Decision\n";
const SPIKES: [&str; 5] = [
    "foundation",
    "mcp-runtime",
    "storage-search",
    "ingestion",
    "documents-git-watch",
];

pub fn provision(root: &Path) -> io::Result<()> {
    write(&root.join("config/plan2-dependencies.txt"), INVENTORY)?;
    write(
        &root.join("config/dependency-decisions.txt"),
        &approved_decisions(),
    )?;
    write(&root.join("docs/dependencies/record.md"), RECORD)?;
    write(
        &root.join("research/results/foundation-acceptance.json"),
        &acceptance(),
    )?;
    for spike in SPIKES {
        write(
            &root.join(format!("research/results/{spike}.json")),
            &result(spike),
        )?;
    }
    Ok(())
}

pub fn approved_decisions() -> String {
    INVENTORY
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once(' '))
        .flat_map(|(_capability, candidates)| {
            candidates.split('|').filter_map(|candidate| {
                let (scope, coordinate) = candidate.split_once(':')?;
                let (name, version) = coordinate.rsplit_once('@')?;
                let decision = if name == "gix" { "reject" } else { "adopt" };
                Some(format!(
                    "{name} {version} {scope} {decision} docs/dependencies/record.md\n"
                ))
            })
        })
        .collect()
}

fn write(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn acceptance() -> String {
    String::from(
        r#"{
  "schema": 1,
  "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "acceptance": "accepted",
  "required_macos_result": {
    "target": "aarch64-apple-darwin",
    "result_spikes": ["foundation", "mcp-runtime", "storage-search", "ingestion", "documents-git-watch"],
    "result_id": "local",
    "status": "passed"
  },
  "plan2_contract": {"status": "accepted"}
}"#,
    )
}

fn result(spike: &str) -> String {
    let failures = if spike == "documents-git-watch" {
        r#"["gix_0_83_0_rejected_malformed_index_panics_in_isolated_process"]"#
    } else {
        "[]"
    };
    format!(
        r#"{{
  "schema": 1,
  "spike": "{spike}",
  "fixture_hash": "blake3:test",
  "dependencies": [],
  "targets": [
    {{"target": "aarch64-apple-darwin", "ci_run_id": "local", "failures": {failures}}}
  ]
}}"#
    )
}
