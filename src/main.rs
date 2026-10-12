use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Planned: ensure one detached publisher is running for this Herdr session.
    Ensure,
    /// Planned: run the metadata publisher in the foreground.
    Daemon,
    /// Planned: stop only this plugin's publisher.
    Stop,
    /// Planned: check the plugin's local setup.
    Doctor,
    /// Planned, opt-in: add sidebar rows to the user's Herdr configuration.
    Configure,
    /// Planned, opt-in: remove only sidebar rows managed by this plugin.
    Unconfigure,
}

fn main() -> ExitCode {
    let _cli = Cli::parse();
    eprintln!("Planned: this command is not implemented yet. No changes were made.");
    ExitCode::FAILURE
}
