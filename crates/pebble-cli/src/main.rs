#![forbid(unsafe_code)]

//! Pebble command-line entry point.

use std::env;
use std::io;
use std::process::ExitCode;

mod arguments;
mod commands;
mod mcp;
mod mcp_response;
mod mcp_schemas;
mod mcp_tools;
mod mcp_transport;
mod mcp_validation;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("pebble failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> io::Result<u8> {
    let arguments = match arguments::parse_from(env::args_os()) {
        Ok(arguments) => arguments,
        Err(error) => {
            let code = if error.use_stderr() { 2 } else { 0 };
            error.print()?;
            return Ok(code);
        }
    };
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable"))?;
    let service = pebble_core::service::PebbleService::open(std::path::Path::new(&home))
        .map_err(io::Error::other)?;
    if matches!(&arguments.command, arguments::Operation::Serve) {
        return mcp::run(service).map(|()| 0);
    }
    commands::dispatch(
        &service,
        arguments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}
