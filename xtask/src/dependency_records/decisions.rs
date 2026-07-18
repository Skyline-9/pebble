use std::collections::BTreeMap;

pub(super) struct DecisionRecord {
    pub(super) version: String,
    pub(super) scope: String,
    pub(super) decision: String,
    pub(super) record_path: String,
}

pub(super) fn parse_decisions(decisions: &str) -> Result<BTreeMap<String, DecisionRecord>, String> {
    let mut records = BTreeMap::new();
    for (line_number, line) in decisions.lines().map(str::trim).enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err(format!(
                "invalid dependency decision on line {}: {line}",
                line_number + 1
            ));
        }
        validate_fields(&fields, line_number + 1)?;
        let name = fields[0].to_owned();
        let record = DecisionRecord {
            version: fields[1].to_owned(),
            scope: fields[2].to_owned(),
            decision: fields[3].to_owned(),
            record_path: fields[4].to_owned(),
        };
        if records.insert(name.clone(), record).is_some() {
            return Err(format!("duplicate dependency decision for {name}"));
        }
    }
    Ok(records)
}

fn validate_fields(fields: &[&str], line_number: usize) -> Result<(), String> {
    let [name, version, scope, decision, _record_path] = fields else {
        return Err(format!(
            "invalid dependency decision field count on line {line_number}"
        ));
    };
    if name.is_empty() {
        return Err(format!(
            "empty dependency name in decision on line {line_number}"
        ));
    }
    if !matches!(
        *scope,
        "production" | "build" | "development" | "research" | "boundary"
    ) {
        return Err(format!(
            "{name} has invalid dependency scope {scope} on line {line_number}"
        ));
    }
    if !matches!(*decision, "adopt" | "reject" | "defer") {
        return Err(format!(
            "{name} has invalid dependency decision {decision} on line {line_number}"
        ));
    }
    if *scope == "boundary" {
        if *version != "n/a" {
            return Err(format!("{name} boundary version must be n/a"));
        }
    } else if !is_exact_registry_version(version) {
        return Err(format!("{name} decision must use an exact version"));
    }
    Ok(())
}

fn is_exact_registry_version(version: &str) -> bool {
    !version.is_empty()
        && version != "n/a"
        && !version.chars().any(|character| {
            character.is_whitespace()
                || matches!(character, '*' | '^' | '~' | '<' | '>' | '=' | ',' | '|')
        })
}
