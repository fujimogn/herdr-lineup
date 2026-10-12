//! Lineup command-line scaffold and planned runtime modules.

pub mod config;
pub mod configure;
pub mod daemon;
pub mod doctor;
pub mod lock;
pub mod model;
pub mod protocol;
pub mod reader_codex;
pub mod reader_grok;
pub mod reader_hermes;
pub mod reader_kimi;
pub mod ship_time;
pub mod socket;
pub mod tokens;

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

/// Run the command-line scaffold.
pub fn run() -> ExitCode {
    let _cli = Cli::parse();
    eprintln!("Planned: this command is not implemented yet. No changes were made.");
    ExitCode::FAILURE
}
