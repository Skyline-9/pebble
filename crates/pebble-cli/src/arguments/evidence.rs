//! Evidence retrieval command schema and parsing: `search` and `read`.

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

use super::{repository_id_command, string};

/// Validated search command fields.
pub struct SearchArguments {
    pub query: String,
    pub repository: String,
    pub budget: u32,
    pub limit: usize,
    pub revision: Option<String>,
    pub path: Option<String>,
    pub language: Option<String>,
    pub kinds: Vec<String>,
}

/// Exact citation read fields.
pub struct ReadArguments {
    pub repository: String,
    pub revision: String,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

pub(super) fn search_schema() -> Command {
    repository_id_command("search", "Retrieve cited evidence")
        .arg(
            Arg::new("query")
                .required(true)
                .value_name("QUERY")
                .index(1),
        )
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
        )
        .arg(Arg::new("revision").long("revision").value_name("REVISION"))
        .arg(Arg::new("path").long("path").value_name("PREFIX"))
        .arg(Arg::new("language").long("language").value_name("LANGUAGE"))
        .arg(
            Arg::new("kind")
                .long("kind")
                .value_name("KIND")
                .action(ArgAction::Append)
                .value_parser(["chunk", "symbol", "file"]),
        )
}

pub(super) fn read_schema() -> Command {
    repository_id_command("read", "Resolve an exact citation")
        .arg(
            Arg::new("revision")
                .long("revision")
                .required(true)
                .value_name("REVISION"),
        )
        .arg(
            Arg::new("path")
                .long("path")
                .required(true)
                .value_name("PATH"),
        )
        .arg(
            Arg::new("start-line")
                .long("start-line")
                .required(true)
                .value_parser(value_parser!(u32).range(1..)),
        )
        .arg(
            Arg::new("end-line")
                .long("end-line")
                .required(true)
                .value_parser(value_parser!(u32).range(1..)),
        )
}

pub(super) fn search(matches: &ArgMatches) -> SearchArguments {
    SearchArguments {
        query: string(matches, "query"),
        repository: string(matches, "repository"),
        budget: *matches.get_one::<u32>("budget").unwrap_or(&6_000),
        limit: usize::try_from(*matches.get_one::<u32>("limit").unwrap_or(&10)).unwrap_or(10),
        revision: matches.get_one::<String>("revision").cloned(),
        path: matches.get_one::<String>("path").cloned(),
        language: matches.get_one::<String>("language").cloned(),
        kinds: matches
            .get_many::<String>("kind")
            .map_or_else(Vec::new, |values| values.cloned().collect()),
    }
}

pub(super) fn read(matches: &ArgMatches) -> ReadArguments {
    ReadArguments {
        repository: string(matches, "repository"),
        revision: string(matches, "revision"),
        path: string(matches, "path"),
        start_line: *matches.get_one::<u32>("start-line").unwrap_or(&1),
        end_line: *matches.get_one::<u32>("end-line").unwrap_or(&1),
    }
}
