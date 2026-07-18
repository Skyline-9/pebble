use super::decisions::DecisionRecord;
use std::collections::BTreeMap;

const REQUIRED_CAPABILITIES: [(&str, &str); 30] = [
    ("serialization", "research:serde@1.0.228"),
    ("json", "research:serde_json@1.0.150"),
    ("errors", "research:thiserror@2.0.18"),
    ("cli", "research:clap@4.6.1"),
    ("runtime", "research:tokio@1.52.3"),
    ("mcp", "research:rmcp@1.8.0"),
    ("graph", "research:rusqlite@0.40.1"),
    ("lexical", "research:tantivy@0.26.1"),
    ("parser", "research:tree-sitter@0.26.8"),
    ("grammar-c", "research:tree-sitter-c@0.24.2"),
    ("grammar-c-sharp", "research:tree-sitter-c-sharp@0.23.5"),
    ("grammar-cpp", "research:tree-sitter-cpp@0.23.4"),
    ("grammar-go", "research:tree-sitter-go@0.25.0"),
    ("grammar-java", "research:tree-sitter-java@0.23.5"),
    (
        "grammar-javascript",
        "research:tree-sitter-javascript@0.25.0",
    ),
    ("grammar-kotlin", "research:tree-sitter-kotlin-ng@1.1.0"),
    ("grammar-python", "research:tree-sitter-python@0.25.0"),
    ("grammar-ruby", "research:tree-sitter-ruby@0.23.1"),
    ("grammar-rust", "research:tree-sitter-rust@0.24.2"),
    ("grammar-swift", "research:tree-sitter-swift@0.7.3"),
    (
        "grammar-typescript",
        "research:tree-sitter-typescript@0.23.2",
    ),
    ("symbols", "research:scip@0.6.1"),
    ("toml", "research:toml@1.1.2"),
    ("markdown", "research:pulldown-cmark@0.13.4"),
    ("yaml", "research:yaml-rust2@0.11.0"),
    ("git", "research:gix@0.83.0|boundary:system-git@n/a"),
    ("traversal", "research:ignore@0.4.28"),
    ("hash", "research:blake3@1.8.5"),
    ("ids", "research:ulid@1.2.1"),
    ("watch", "research:notify@8.2.0"),
];

/// Require the complete, exact Plan 2 capability inventory.
///
/// # Errors
///
/// Returns an error for an empty, missing, unknown, duplicate, malformed, or
/// altered inventory entry.
pub(super) fn validate_inventory(requirements: &str) -> Result<(), String> {
    let expected = REQUIRED_CAPABILITIES
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut actual = BTreeMap::new();

    for line in requirements.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (capability, candidates) = parse_requirement(line)?;
        if !expected.contains_key(capability) {
            return Err(format!("unknown Plan 2 capability: {capability}"));
        }
        if actual.insert(capability, candidates).is_some() {
            return Err(format!("duplicate Plan 2 capability: {capability}"));
        }
    }

    if actual.is_empty() {
        return Err(String::from("Plan 2 capability inventory cannot be empty"));
    }

    let missing = expected
        .keys()
        .filter(|capability| !actual.contains_key(**capability))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "missing Plan 2 capabilities: {}",
            missing.join(", ")
        ));
    }

    for (capability, expected_candidates) in expected {
        if actual[capability] != expected_candidates {
            return Err(format!("incorrect candidates for {capability}"));
        }
    }
    Ok(())
}

/// Return Plan 2 capabilities without exactly one approved alternative.
///
/// # Errors
///
/// Returns an error when the capability contract or decision registry is
/// malformed.
pub(super) fn invalid_requirements(
    records: &BTreeMap<String, DecisionRecord>,
    requirements: &str,
) -> Result<Vec<String>, String> {
    let mut invalid = Vec::new();

    for line in requirements.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (capability, candidates) = parse_requirement(line)?;

        let selected = candidates
            .split('|')
            .map(|candidate| approved_candidate(records, candidate))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|selected| *selected)
            .count();
        if selected != 1 {
            invalid.push(capability.to_owned());
        }
    }

    Ok(invalid)
}

fn parse_requirement(line: &str) -> Result<(&str, &str), String> {
    let (capability, candidates) = line
        .split_once(' ')
        .ok_or_else(|| format!("invalid Plan 2 requirement: {line}"))?;
    if capability.is_empty()
        || candidates.is_empty()
        || capability.contains(char::is_whitespace)
        || candidates.contains(char::is_whitespace)
    {
        return Err(format!("invalid Plan 2 requirement: {line}"));
    }
    Ok((capability, candidates))
}

fn approved_candidate(
    records: &BTreeMap<String, DecisionRecord>,
    candidate: &str,
) -> Result<bool, String> {
    let (scope, coordinate) = candidate
        .split_once(':')
        .ok_or_else(|| format!("invalid Plan 2 scope: {candidate}"))?;
    if !matches!(scope, "research" | "boundary") {
        return Err(format!("unapproved Plan 2 scope: {scope}"));
    }
    let (name, version) = coordinate
        .rsplit_once('@')
        .ok_or_else(|| format!("invalid Plan 2 candidate: {candidate}"))?;
    if name.is_empty() || version.is_empty() || name.contains('@') {
        return Err(format!("invalid Plan 2 candidate: {candidate}"));
    }

    Ok(records.get(name).is_some_and(|record| {
        let approved_scope =
            record.scope == scope || (scope == "research" && record.scope == "production");
        approved_scope && record.decision == "adopt" && record.version == version
    }))
}
