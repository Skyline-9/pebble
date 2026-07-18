//! Bounded clap command schema for Plan 2.

mod evidence;
mod plan2;

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

pub use evidence::{ReadArguments, SearchArguments};

/// Fully parsed model-free command invocation.
pub struct Arguments {
    /// Emit machine-readable JSON rather than human text.
    pub json: bool,
    /// Selected operation.
    pub command: Operation,
}

/// Supported Plan 2 operations.
pub enum Operation {
    /// Initialize portable repository configuration.
    Init { repository: PathBuf },
    /// Register one local checkout.
    Register {
        repository: PathBuf,
        alternate_worktree: bool,
    },
    /// Compile one immutable generation.
    Index { repository: PathBuf },
    /// Watch and reconcile a registered checkout.
    Watch { repository: PathBuf, once: bool },
    /// Retrieve cited model-free evidence.
    Search(SearchArguments),
    /// Resolve one exact citation.
    Read(ReadArguments),
    /// Validate one disposable projection.
    Health { repository: String },
    /// List bounded local retrieval traces.
    Traces { repository: String, limit: usize },
    /// Build a fresh disposable projection.
    Rebuild { repository: PathBuf },
    /// Show the install disclosure for, or install, a local embedding model.
    ModelInstall { model_id: String, confirm: bool },
    /// List every installed embedding model.
    ModelList,
    /// Select the active embedding model.
    ModelSelect { model_id: String },
    /// Remove one installed embedding model.
    ModelRemove { model_id: String },
    /// List managed living-knowledge claims.
    NoteList {
        repository: String,
        status: Option<String>,
    },
    /// Read one managed living-knowledge claim.
    NoteRead {
        repository: String,
        claim_id: String,
    },
    /// List queued living-note update packets.
    UpdateList { repository: String },
    /// Apply one queued living-note update packet.
    UpdateApply {
        repository: String,
        claim_id: String,
        patch_file: PathBuf,
    },
    /// Create a new multi-repository workspace.
    WorkspaceCreate { name: String },
    /// Add a registered repository to a workspace.
    WorkspaceAdd { name: String, repository_id: String },
    /// List every workspace.
    WorkspaceList,
    /// Search every present repository in a workspace.
    WorkspaceSearch {
        name: String,
        query: String,
        budget: u32,
        limit: usize,
    },
    /// Create a new personal knowledge note.
    PersonalCreate { title: String },
    /// List every personal knowledge note.
    PersonalList,
    /// Promote a personal note into a registered repository.
    PersonalPromote {
        note_id: String,
        repository: String,
        confirm: bool,
        overwrite: bool,
    },
    /// Start the bounded stdio MCP server supplied by the adapter task.
    Serve,
}

/// Parse one invocation with clap's bounded value parsers.
pub fn parse_from<I, T>(arguments: I) -> Result<Arguments, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = schema().try_get_matches_from(arguments)?;
    let json = matches.get_flag("json");
    let (name, command) = matches.subcommand().ok_or_else(|| {
        schema().error(
            clap::error::ErrorKind::MissingSubcommand,
            "a command is required",
        )
    })?;
    let command = match name {
        "init" => Operation::Init {
            repository: path(command, "repository"),
        },
        "register" => Operation::Register {
            repository: path(command, "repository"),
            alternate_worktree: command.get_flag("alternate-worktree"),
        },
        "index" => Operation::Index {
            repository: path(command, "repository"),
        },
        "watch" => Operation::Watch {
            repository: path(command, "repository"),
            once: command.get_flag("once"),
        },
        "search" => Operation::Search(evidence::search(command)),
        "read" => Operation::Read(evidence::read(command)),
        "health" => Operation::Health {
            repository: string(command, "repository"),
        },
        "traces" => Operation::Traces {
            repository: string(command, "repository"),
            limit: usize::try_from(*command.get_one::<u32>("limit").unwrap_or(&20)).unwrap_or(20),
        },
        "rebuild" => Operation::Rebuild {
            repository: path(command, "repository"),
        },
        "model" => plan2::model_operation(command),
        "note" => plan2::note_operation(command),
        "update" => plan2::update_operation(command),
        "workspace" => plan2::workspace_operation(command),
        "personal" => plan2::personal_operation(command),
        "serve" => Operation::Serve,
        _ => unreachable!("clap only yields declared subcommands"),
    };
    Ok(Arguments { json, command })
}

fn schema() -> Command {
    Command::new("pebble")
        .version(pebble_core::VERSION)
        .about("Local model-free repository evidence")
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Emit machine-readable JSON on stdout"),
        )
        .subcommand_required(true)
        .subcommand(command_with_repository(
            "init",
            "Initialize repository configuration",
        ))
        .subcommand(
            command_with_repository("register", "Register a local checkout").arg(
                Arg::new("alternate-worktree")
                    .long("alternate-worktree")
                    .action(ArgAction::SetTrue),
            ),
        )
        .subcommand(command_with_repository(
            "index",
            "Compile and activate an index",
        ))
        .subcommand(
            command_with_repository("watch", "Watch and reconcile a checkout").arg(
                Arg::new("once")
                    .long("once")
                    .action(ArgAction::SetTrue)
                    .help("Request one reconciliation and exit"),
            ),
        )
        .subcommand(evidence::search_schema())
        .subcommand(evidence::read_schema())
        .subcommand(repository_id_command(
            "health",
            "Validate the current index",
        ))
        .subcommand(
            repository_id_command("traces", "List local retrieval traces").arg(
                Arg::new("limit")
                    .long("limit")
                    .default_value("20")
                    .value_parser(value_parser!(u32).range(1..=1_000)),
            ),
        )
        .subcommand(command_with_repository(
            "rebuild",
            "Build a fresh disposable projection",
        ))
        .subcommand(plan2::model_schema())
        .subcommand(plan2::note_schema())
        .subcommand(plan2::update_schema())
        .subcommand(plan2::workspace_schema())
        .subcommand(plan2::personal_schema())
        .subcommand(Command::new("serve").about("Serve model-free operations over stdio MCP"))
}

fn command_with_repository(name: &'static str, about: &'static str) -> Command {
    Command::new(name).about(about).arg(
        Arg::new("repository")
            .value_name("PATH")
            .default_value(".")
            .value_parser(value_parser!(PathBuf)),
    )
}

fn repository_id_command(name: &'static str, about: &'static str) -> Command {
    Command::new(name).about(about).arg(
        Arg::new("repository")
            .long("repository")
            .required(true)
            .value_name("ID"),
    )
}

fn string(matches: &ArgMatches, name: &str) -> String {
    matches.get_one::<String>(name).cloned().unwrap_or_default()
}

fn path(matches: &ArgMatches, name: &str) -> PathBuf {
    matches
        .get_one::<PathBuf>(name)
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}
