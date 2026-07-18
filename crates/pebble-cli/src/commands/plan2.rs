//! Dispatch and helpers for embedding models, living knowledge, workspaces,
//! and personal notes.

use std::io::Write;
use std::path::Path;

use pebble_core::service::{PebbleService, ServiceError};

use crate::arguments::Operation;

use super::{operational, output, repository_id, usage};

/// Largest queued-update patch file the CLI will read from disk.
const MAX_PATCH_FILE_BYTES: u64 = 1024 * 1024;

pub(super) fn dispatch_plan2(
    service: &PebbleService,
    operation: Operation,
    json: bool,
    stdout: &mut impl Write,
) -> Result<u8, ServiceError> {
    match operation {
        Operation::ModelInstall { model_id, confirm } => {
            model_install(service, &model_id, confirm, json, stdout)?;
        }
        Operation::ModelList => model_list(service, json, stdout)?,
        Operation::ModelSelect { model_id } => model_select(service, &model_id, json, stdout)?,
        Operation::ModelRemove { model_id } => model_remove(service, &model_id, json, stdout)?,
        Operation::NoteList { repository, status } => {
            note_list(service, &repository, status.as_deref(), json, stdout)?;
        }
        Operation::NoteRead {
            repository,
            claim_id,
        } => {
            note_read(service, &repository, &claim_id, json, stdout)?;
        }
        Operation::UpdateList { repository } => update_list(service, &repository, json, stdout)?,
        Operation::UpdateApply {
            repository,
            claim_id,
            patch_file,
        } => update_apply(service, &repository, &claim_id, &patch_file, json, stdout)?,
        Operation::WorkspaceCreate { name } => workspace_create(service, &name, json, stdout)?,
        Operation::WorkspaceAdd {
            name,
            repository_id,
        } => {
            workspace_add(service, &name, &repository_id, json, stdout)?;
        }
        Operation::WorkspaceList => workspace_list(service, json, stdout)?,
        Operation::WorkspaceSearch {
            name,
            query,
            budget,
            limit,
        } => workspace_search(service, &name, &query, (budget, limit), json, stdout)?,
        Operation::PersonalCreate { title } => personal_create(service, &title, json, stdout)?,
        Operation::PersonalList => personal_list(service, json, stdout)?,
        Operation::PersonalPromote {
            note_id,
            repository,
            confirm,
            overwrite,
        } => personal_promote(
            service,
            &note_id,
            &repository,
            (confirm, overwrite),
            json,
            stdout,
        )?,
        _ => unreachable!("execute() handles all other operations"),
    }
    Ok(0)
}

fn model_install(
    service: &PebbleService,
    model_id: &str,
    confirm: bool,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let result = service.model_install(model_id, confirm)?;
    output(stdout, json, &result, || {
        if result.installed {
            format!("installed model {}", result.model_id)
        } else {
            result
                .disclosure
                .clone()
                .unwrap_or_else(|| format!("model {} requires confirmation", result.model_id))
        }
    })
}

fn model_list(
    service: &PebbleService,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let models = service.model_list()?;
    output(stdout, json, &models, || {
        format!("{} installed model(s)", models.len())
    })
}

fn model_select(
    service: &PebbleService,
    model_id: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let manifest = service.model_select(model_id)?;
    output(stdout, json, &manifest, || {
        format!("selected model {}", manifest.id)
    })
}

fn model_remove(
    service: &PebbleService,
    model_id: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let result = service.model_remove(model_id)?;
    output(stdout, json, &result, || {
        format!("removed model {}", result.model_id)
    })
}

fn note_list(
    service: &PebbleService,
    repository: &str,
    status: Option<&str>,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let repository = repository_id(repository)?;
    let claims = service.note_list(&repository, status)?;
    output(stdout, json, &claims, || {
        format!("{} managed claim(s)", claims.len())
    })
}

fn note_read(
    service: &PebbleService,
    repository: &str,
    claim_id: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let repository = repository_id(repository)?;
    let claim = service.note_read(&repository, claim_id)?;
    output(stdout, json, &claim, || claim.body.clone())
}

fn update_list(
    service: &PebbleService,
    repository: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let repository = repository_id(repository)?;
    let updates = service.update_list(&repository)?;
    output(stdout, json, &updates, || {
        format!("{} queued update(s)", updates.len())
    })
}

fn update_apply(
    service: &PebbleService,
    repository: &str,
    claim_id: &str,
    patch_file: &Path,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let repository = repository_id(repository)?;
    let patch = read_patch_file(patch_file)?;
    let applied = service.update_apply(&repository, claim_id, &patch)?;
    output(stdout, json, &applied, || {
        format!("applied update to claim {}", applied.claim_id)
    })
}

fn workspace_create(
    service: &PebbleService,
    name: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let workspace = service.workspace_create(name)?;
    output(stdout, json, &workspace, || {
        format!("created workspace {}", workspace.name)
    })
}

fn workspace_add(
    service: &PebbleService,
    name: &str,
    repository: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let repository = repository_id(repository)?;
    let workspace = service.workspace_add_repository(name, &repository)?;
    output(stdout, json, &workspace, || {
        format!(
            "workspace {} now has {} repositor(y/ies)",
            workspace.name,
            workspace.repositories.len()
        )
    })
}

fn workspace_list(
    service: &PebbleService,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let workspaces = service.workspace_list()?;
    output(stdout, json, &workspaces, || {
        format!("{} workspace(s)", workspaces.len())
    })
}

fn workspace_search(
    service: &PebbleService,
    name: &str,
    query: &str,
    budget_and_limit: (u32, usize),
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let (budget, limit) = budget_and_limit;
    let outcome = service.workspace_search(name, query, budget, limit)?;
    output(stdout, json, &outcome, || {
        format!(
            "{} hit(s), {} unresolved repositor(y/ies)",
            outcome.hits.len(),
            outcome.unresolved.len()
        )
    })
}

fn personal_create(
    service: &PebbleService,
    title: &str,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let note = service.personal_note_create(title)?;
    output(stdout, json, &note, || {
        format!("created personal note {}", note.id)
    })
}

fn personal_list(
    service: &PebbleService,
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let notes = service.personal_note_list()?;
    output(stdout, json, &notes, || {
        format!("{} personal note(s)", notes.len())
    })
}

fn personal_promote(
    service: &PebbleService,
    note_id: &str,
    repository: &str,
    confirm_and_overwrite: (bool, bool),
    json: bool,
    stdout: &mut impl Write,
) -> Result<(), ServiceError> {
    let (confirm, overwrite) = confirm_and_overwrite;
    let repository = repository_id(repository)?;
    let outcome = service.personal_note_promote(note_id, &repository, confirm, overwrite)?;
    output(stdout, json, &outcome, || {
        if outcome.applied {
            format!("promoted note to {}", outcome.destination_path.display())
        } else {
            outcome.diff.clone().unwrap_or_else(|| {
                format!(
                    "promotion to {} requires confirmation",
                    outcome.destination_path.display()
                )
            })
        }
    })
}

fn read_patch_file(path: &Path) -> Result<String, ServiceError> {
    let metadata = std::fs::metadata(path).map_err(operational)?;
    if metadata.len() > MAX_PATCH_FILE_BYTES {
        return Err(usage(format!(
            "patch file exceeds the {MAX_PATCH_FILE_BYTES} byte limit"
        )));
    }
    std::fs::read_to_string(path).map_err(operational)
}
