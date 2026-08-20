use std::env;
use std::process::ExitCode;

use active_work::{Cli, Command, Store, execute, serve, watch};
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let store = match Store::from_environment() {
        Ok(store) => store,
        Err(error) => return fail(error),
    };
    let current_dir = match env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(error) => return fail(error.into()),
    };

    match cli.command {
        Some(Command::Serve { port }) => match serve::run(&store, port) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(error),
        },
        Some(Command::Watch { interval, all }) => match watch::run(&store, interval, all) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(error),
        },
        command => match execute(command, &store, &current_dir) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
                ExitCode::SUCCESS
            }
            Err(error) => fail(error),
        },
    }
}

fn fail(error: anyhow::Error) -> ExitCode {
    eprintln!("aw: {error:#}");
    ExitCode::FAILURE
}
