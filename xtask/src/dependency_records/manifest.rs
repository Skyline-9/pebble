use super::{path_policy, syntax};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn workspace_dependencies(manifest: &str) -> Result<BTreeMap<String, String>, String> {
    let mut in_section = false;
    let mut dependencies = BTreeMap::new();

    for raw_line in manifest.lines() {
        let line = syntax::without_comment(raw_line).trim();
        if line.starts_with('[') {
            if syntax::contains_unicode_escape(line) {
                return Err(format!("escaped dependency table is forbidden: {line}"));
            }
            if line != "[workspace.dependencies]" && dependency_table_like(line) {
                return Err(format!("multiline dependency entry: {line}"));
            }
            in_section = line == "[workspace.dependencies]";
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if escaped_assignment_name(line) {
            return Err(format!("escaped dependency key is forbidden: {line}"));
        }
        if !in_section {
            reject_dotted_dependency_assignment(line)?;
            continue;
        }
        let (name, requirement) = assignment(line)
            .ok_or_else(|| format!("multiline or invalid dependency entry: {line}"))?;
        validate_name(name)?;
        let version = workspace_version(name, requirement)?;
        if dependencies.insert(name.to_owned(), version).is_some() {
            return Err(format!("duplicate workspace dependency: {name}"));
        }
    }
    Ok(dependencies)
}

pub(super) fn member_manifest_violations(
    manifest: &str,
    member_dir: &Path,
    repository_root: &Path,
) -> Vec<String> {
    let mut in_section = false;
    let mut violations = Vec::new();

    for raw_line in manifest.lines() {
        let line = syntax::without_comment(raw_line).trim();
        if line.starts_with('[') {
            if syntax::contains_unicode_escape(line) {
                violations.push(format!("escaped dependency table is forbidden: {line}"));
            }
            if !is_dependency_section(line) && dependency_table_like(line) {
                violations.push(format!("multiline dependency entry: {line}"));
            }
            in_section = is_dependency_section(line);
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if escaped_assignment_name(line) {
            violations.push(format!("escaped dependency key is forbidden: {line}"));
            continue;
        }
        if !in_section {
            if dotted_dependency_assignment(line) {
                violations.push(format!("multiline dependency entry: {line}"));
            }
            continue;
        }
        let Some((name, requirement)) = assignment(line) else {
            violations.push(format!("multiline dependency entry: {line}"));
            continue;
        };
        if let Err(error) = validate_name(name) {
            violations.push(error);
            continue;
        }
        check_member_requirement(
            name,
            requirement,
            member_dir,
            repository_root,
            &mut violations,
        );
    }
    violations
}

fn workspace_version(name: &str, requirement: &str) -> Result<String, String> {
    if let Some(value) = quoted_value(requirement) {
        return exact_version(value).ok_or_else(|| format!("{name} must use an exact version"));
    }
    let fields = inline_fields(requirement)
        .map_err(|error| format!("{name} has invalid inline dependency: {error}"))?;
    for source in ["git", "path", "registry"] {
        if field_value(&fields, source).is_some() {
            let label = if source == "git" { "Git" } else { source };
            return Err(format!(
                "{label} dependency source is forbidden for workspace dependency {name}"
            ));
        }
    }
    let version = field_value(&fields, "version")
        .and_then(quoted_value)
        .and_then(exact_version)
        .ok_or_else(|| format!("{name} must use an exact version"))?;
    Ok(version)
}

fn check_member_requirement(
    name: &str,
    requirement: &str,
    member_dir: &Path,
    repository_root: &Path,
    violations: &mut Vec<String>,
) {
    let Ok(fields) = inline_fields(requirement) else {
        violations.push(format!(
            "multiline dependency entry: {name} = {requirement}"
        ));
        return;
    };
    if fields.is_empty() {
        violations.push(format!("{name} must use workspace = true"));
        return;
    }
    if field_value(&fields, "git").is_some() {
        violations.push(format!("Git dependency is forbidden: {name}"));
        return;
    }
    if field_value(&fields, "registry").is_some() {
        violations.push(format!("registry source is forbidden: {name}"));
        return;
    }
    if let Some(path) = field_value(&fields, "path").and_then(quoted_value) {
        path_policy::check(name, path, member_dir, repository_root, violations);
        return;
    }
    if field_value(&fields, "version").is_some()
        || field_value(&fields, "workspace") != Some("true")
    {
        violations.push(format!("{name} must use workspace = true"));
    }
}

fn is_dependency_section(header: &str) -> bool {
    matches!(
        header,
        "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
    ) || (header.starts_with("[target.")
        && (header.ends_with(".dependencies]")
            || header.ends_with(".dev-dependencies]")
            || header.ends_with(".build-dependencies]")))
}

fn dependency_table_like(header: &str) -> bool {
    header.contains("dependencies")
}

fn reject_dotted_dependency_assignment(line: &str) -> Result<(), String> {
    if dotted_dependency_assignment(line) {
        Err(format!("multiline dependency entry: {line}"))
    } else {
        Ok(())
    }
}

fn dotted_dependency_assignment(line: &str) -> bool {
    assignment(line).is_some_and(|(name, _)| name.contains("dependencies"))
}

fn escaped_assignment_name(line: &str) -> bool {
    assignment(line).is_some_and(|(name, _)| syntax::contains_unicode_escape(name))
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    let (name, requirement) = line.split_once('=')?;
    let name = name.trim();
    let requirement = requirement.trim();
    if name.is_empty() || requirement.is_empty() {
        None
    } else {
        Some((name, requirement))
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        Ok(())
    } else {
        Err(format!("invalid workspace dependency name: {name}"))
    }
}

fn exact_version(value: &str) -> Option<String> {
    let version = value.strip_prefix('=')?;
    if version.is_empty()
        || version.chars().any(|character| {
            character.is_whitespace()
                || matches!(character, '*' | '^' | '~' | '<' | '>' | '=' | ',' | '|')
        })
    {
        None
    } else {
        Some(version.to_owned())
    }
}

fn inline_fields(requirement: &str) -> Result<Vec<(&str, &str)>, String> {
    let Some(body) = requirement
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    else {
        return Ok(Vec::new());
    };
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut bracket_depth = 0_u16;

    for (index, character) in body.char_indices() {
        if let Some(delimiter) = quote {
            if character == delimiter && !escaped {
                quote = None;
            }
            escaped = character == '\\' && !escaped;
            if character != '\\' {
                escaped = false;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '[' => bracket_depth = bracket_depth.saturating_add(1),
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ',' if bracket_depth == 0 => {
                push_field(&body[start..index], &mut fields)?;
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quote.is_some() || bracket_depth != 0 {
        return Err(String::from("unterminated inline value"));
    }
    push_field(&body[start..], &mut fields)?;
    Ok(fields)
}

fn push_field<'a>(field: &'a str, output: &mut Vec<(&'a str, &'a str)>) -> Result<(), String> {
    let field = field.trim();
    if field.is_empty() {
        return Ok(());
    }
    let (name, value) =
        assignment(field).ok_or_else(|| format!("invalid inline dependency field: {field}"))?;
    validate_name(name)?;
    if output.iter().any(|(existing, _)| *existing == name) {
        return Err(format!("duplicate inline dependency field: {name}"));
    }
    output.push((name, value));
    Ok(())
}

fn field_value<'a>(fields: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|(field, value)| (*field == name).then_some(*value))
}

fn quoted_value(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
}
