#[path = "foundation/json.rs"]
mod json;

use json::{Json, JsonParser};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const MACOS_TARGET: &str = "aarch64-apple-darwin";
const LOCAL_RESULT_ID: &str = "local";
const RESULT_SPIKES: [&str; 5] = [
    "foundation",
    "mcp-runtime",
    "storage-search",
    "ingestion",
    "documents-git-watch",
];
const DOCUMENTS_GIT_WATCH: &str = "documents-git-watch";
const GIX_REJECTION: &str = "gix_0_83_0_rejected_malformed_index_panics_in_isolated_process";

pub(super) fn validate(root: &Path, capabilities_are_approved: bool) -> Result<(), String> {
    let path = root.join("research/results/foundation-acceptance.json");
    let value = read_json(&path)?;
    let evidence = json_object(&value, "foundation acceptance")?;
    required_number(evidence, "schema", "foundation acceptance", "1")?;
    let commit = required_string(evidence, "commit", "foundation acceptance")?;
    if !is_commit(commit) {
        return Err(String::from("foundation acceptance commit must be a SHA-1"));
    }
    let acceptance = required_string(evidence, "acceptance", "foundation acceptance")?;
    let plan2 = required_object(evidence, "plan2_contract", "foundation acceptance")?;
    let plan2_status = required_string(plan2, "status", "plan2_contract")?;
    let macos = required_object(evidence, "required_macos_result", "foundation acceptance")?;
    if required_string(macos, "target", "required_macos_result")? != MACOS_TARGET {
        return Err(String::from(
            "Plan 2 acceptance must require aarch64-apple-darwin evidence",
        ));
    }
    required_strings(
        macos,
        "result_spikes",
        "required_macos_result",
        &RESULT_SPIKES,
    )?;
    let result_id = required_string(macos, "result_id", "required_macos_result")?;
    if result_id != LOCAL_RESULT_ID {
        return Err(String::from(
            "Plan 2 acceptance must use the checked-in local macOS result ID",
        ));
    }
    let macos_status = required_string(macos, "status", "required_macos_result")?;

    match capabilities_are_approved {
        false
            if acceptance == "blocked"
                && plan2_status == "blocked"
                && macos_status == "pending" =>
        {
            Ok(())
        }
        false => Err(String::from(
            "Plan 2 acceptance evidence must remain blocked until every capability is approved",
        )),
        true if acceptance == "accepted"
            && plan2_status == "accepted"
            && macos_status == "passed" =>
        {
            for spike in RESULT_SPIKES {
                validate_result(root, spike, result_id)?;
            }
            Ok(())
        }
        true => Err(String::from(
            "approved Plan 2 capabilities require accepted local macOS evidence",
        )),
    }
}

fn validate_result(root: &Path, spike: &str, result_id: &str) -> Result<(), String> {
    let path = root.join(format!("research/results/{spike}.json"));
    let value = read_json(&path)?;
    let result = json_object(&value, spike)?;
    if required_string(result, "spike", spike)? != spike {
        return Err(format!("{spike} evidence has an incorrect spike name"));
    }
    let targets = required_array(result, "targets", spike)?;
    if targets.len() != 1 {
        return Err(format!(
            "{spike} evidence must contain exactly one macOS result"
        ));
    }
    let result_target = json_object(&targets[0], "macOS result")?;
    if required_string(result_target, "target", spike)? != MACOS_TARGET {
        return Err(format!("{spike} evidence must contain a macOS result"));
    }
    if required_string(result_target, "ci_run_id", spike)? != result_id {
        return Err(format!(
            "{spike} evidence does not match local macOS result ID"
        ));
    }
    let failures = required_array(result_target, "failures", spike)?;
    if spike == DOCUMENTS_GIT_WATCH {
        return required_strings(result_target, "failures", spike, &[GIX_REJECTION]).map_err(
            |_| String::from("documents-git-watch evidence must preserve the gix rejection"),
        );
    }
    if !failures.is_empty() {
        return Err(format!(
            "{spike} evidence contains probe failures for {MACOS_TARGET}"
        ));
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Json, String> {
    let text =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    JsonParser::new(&text)
        .parse()
        .map_err(|error| format!("parse {}: {error}", path.display()))
}

fn json_object<'a>(value: &'a Json, context: &str) -> Result<&'a BTreeMap<String, Json>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{context} must be a JSON object"))
}

fn required_object<'a>(
    fields: &'a BTreeMap<String, Json>,
    key: &str,
    context: &str,
) -> Result<&'a BTreeMap<String, Json>, String> {
    json_object(
        fields
            .get(key)
            .ok_or_else(|| format!("{context} is missing {key}"))?,
        context,
    )
}

fn required_array<'a>(
    object: &'a BTreeMap<String, Json>,
    key: &str,
    context: &str,
) -> Result<&'a [Json], String> {
    object
        .get(key)
        .and_then(Json::as_array)
        .ok_or_else(|| format!("{context} must contain array {key}"))
}

fn required_string<'a>(
    object: &'a BTreeMap<String, Json>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Json::as_string)
        .ok_or_else(|| format!("{context} must contain string {key}"))
}

fn required_number(
    object: &BTreeMap<String, Json>,
    key: &str,
    context: &str,
    expected: &str,
) -> Result<(), String> {
    if object.get(key).and_then(Json::as_number) == Some(expected) {
        Ok(())
    } else {
        Err(format!("{context} must contain number {key}={expected}"))
    }
}

fn required_strings(
    object: &BTreeMap<String, Json>,
    key: &str,
    context: &str,
    expected: &[&str],
) -> Result<(), String> {
    let values = required_array(object, key, context)?;
    let actual = values
        .iter()
        .map(|value| value.as_string().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("{context} must contain string array {key}"))?;
    if actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        Ok(())
    } else {
        Err(format!("{context} has an incorrect {key} inventory"))
    }
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|character| character.is_ascii_hexdigit())
}
