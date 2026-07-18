//! Command schema and parsing for embedding models, living knowledge,
//! workspaces, and personal notes.

use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

use super::{Operation, repository_id_command, string};

pub(super) fn model_operation(matches: &ArgMatches) -> Operation {
    let Some((name, command)) = matches.subcommand() else {
        unreachable!("clap only yields declared subcommands")
    };
    match name {
        "install" => Operation::ModelInstall {
            model_id: string(command, "model"),
            confirm: command.get_flag("yes"),
        },
        "list" => Operation::ModelList,
        "select" => Operation::ModelSelect {
            model_id: string(command, "model"),
        },
        "remove" => Operation::ModelRemove {
            model_id: string(command, "model"),
        },
        _ => unreachable!("clap only yields declared subcommands"),
    }
}

pub(super) fn note_operation(matches: &ArgMatches) -> Operation {
    let Some((name, command)) = matches.subcommand() else {
        unreachable!("clap only yields declared subcommands")
    };
    match name {
        "list" => Operation::NoteList {
            repository: string(command, "repository"),
            status: command.get_one::<String>("status").cloned(),
        },
        "read" => Operation::NoteRead {
            repository: string(command, "repository"),
            claim_id: string(command, "claim-id"),
        },
        _ => unreachable!("clap only yields declared subcommands"),
    }
}

pub(super) fn update_operation(matches: &ArgMatches) -> Operation {
    let Some((name, command)) = matches.subcommand() else {
        unreachable!("clap only yields declared subcommands")
    };
    match name {
        "list" => Operation::UpdateList {
            repository: string(command, "repository"),
        },
        "apply" => Operation::UpdateApply {
            repository: string(command, "repository"),
            claim_id: string(command, "claim-id"),
            patch_file: super::path(command, "patch-file"),
        },
        _ => unreachable!("clap only yields declared subcommands"),
    }
}

pub(super) fn workspace_operation(matches: &ArgMatches) -> Operation {
    let Some((name, command)) = matches.subcommand() else {
        unreachable!("clap only yields declared subcommands")
    };
    match name {
        "create" => Operation::WorkspaceCreate {
            name: string(command, "name"),
        },
        "add" => Operation::WorkspaceAdd {
            name: string(command, "name"),
            repository_id: string(command, "repository-id"),
        },
        "list" => Operation::WorkspaceList,
        "search" => Operation::WorkspaceSearch {
            name: string(command, "name"),
            query: string(command, "query"),
            budget: *command.get_one::<u32>("budget").unwrap_or(&6_000),
            limit: usize::try_from(*command.get_one::<u32>("limit").unwrap_or(&10)).unwrap_or(10),
        },
        _ => unreachable!("clap only yields declared subcommands"),
    }
}

pub(super) fn personal_operation(matches: &ArgMatches) -> Operation {
    let Some((name, command)) = matches.subcommand() else {
        unreachable!("clap only yields declared subcommands")
    };
    match name {
        "create" => Operation::PersonalCreate {
            title: string(command, "title"),
        },
        "list" => Operation::PersonalList,
        "promote" => Operation::PersonalPromote {
            note_id: string(command, "note-id"),
            repository: string(command, "repository"),
            confirm: command.get_flag("yes"),
            overwrite: command.get_flag("overwrite"),
        },
        _ => unreachable!("clap only yields declared subcommands"),
    }
}

pub(super) fn model_schema() -> Command {
    Command::new("model")
        .about("Manage local embedding models")
        .subcommand_required(true)
        .subcommand(
            Command::new("install")
                .about("Show the install disclosure, or install, a model")
                .arg(Arg::new("model").required(true).value_name("MODEL"))
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .action(ArgAction::SetTrue)
                        .help("Confirm the install disclosure and download the model"),
                ),
        )
        .subcommand(Command::new("list").about("List every installed model"))
        .subcommand(
            Command::new("select")
                .about("Select the active model")
                .arg(Arg::new("model").required(true).value_name("MODEL")),
        )
        .subcommand(
            Command::new("remove")
                .about("Remove one installed model")
                .arg(Arg::new("model").required(true).value_name("MODEL")),
        )
}

pub(super) fn note_schema() -> Command {
    Command::new("note")
        .about("Read managed living-knowledge claims")
        .subcommand_required(true)
        .subcommand(
            repository_id_command("list", "List managed claims").arg(
                Arg::new("status")
                    .long("status")
                    .value_name("STATUS")
                    .value_parser(["current", "stale", "pending_update", "broken"]),
            ),
        )
        .subcommand(
            repository_id_command("read", "Read one managed claim")
                .arg(Arg::new("claim-id").required(true).value_name("CLAIM_ID")),
        )
}

pub(super) fn update_schema() -> Command {
    Command::new("update")
        .about("Manage queued living-note update packets")
        .subcommand_required(true)
        .subcommand(repository_id_command("list", "List queued update packets"))
        .subcommand(
            repository_id_command("apply", "Apply one queued update packet")
                .arg(Arg::new("claim-id").required(true).value_name("CLAIM_ID"))
                .arg(
                    Arg::new("patch-file")
                        .long("patch-file")
                        .required(true)
                        .value_name("PATH")
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
}

pub(super) fn workspace_schema() -> Command {
    Command::new("workspace")
        .about("Manage multi-repository workspaces")
        .subcommand_required(true)
        .subcommand(
            Command::new("create")
                .about("Create a new empty workspace")
                .arg(Arg::new("name").required(true).value_name("NAME")),
        )
        .subcommand(
            Command::new("add")
                .about("Add a registered repository to a workspace")
                .arg(Arg::new("name").required(true).value_name("NAME"))
                .arg(
                    Arg::new("repository-id")
                        .required(true)
                        .value_name("REPOSITORY_ID"),
                ),
        )
        .subcommand(Command::new("list").about("List every workspace"))
        .subcommand(
            Command::new("search")
                .about("Search every present repository in a workspace")
                .arg(Arg::new("name").required(true).value_name("NAME"))
                .arg(Arg::new("query").required(true).value_name("QUERY"))
                .arg(
                    Arg::new("budget")
                        .long("budget")
                        .default_value("6000")
                        .value_parser(value_parser!(u32).range(1_000..=32_000)),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .default_value("10")
                        .value_parser(value_parser!(u32).range(1..=100)),
                ),
        )
}

pub(super) fn personal_schema() -> Command {
    Command::new("personal")
        .about("Manage personal knowledge notes")
        .subcommand_required(true)
        .subcommand(
            Command::new("create")
                .about("Create a new personal note")
                .arg(Arg::new("title").required(true).value_name("TITLE")),
        )
        .subcommand(Command::new("list").about("List every personal note"))
        .subcommand(
            Command::new("promote")
                .about("Promote a personal note into a registered repository")
                .arg(Arg::new("note-id").required(true).value_name("NOTE_ID"))
                .arg(
                    Arg::new("repository")
                        .long("repository")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .action(ArgAction::SetTrue)
                        .help("Confirm the write"),
                )
                .arg(
                    Arg::new("overwrite")
                        .long("overwrite")
                        .action(ArgAction::SetTrue)
                        .help("Acknowledge overwriting different existing content"),
                ),
        )
}
