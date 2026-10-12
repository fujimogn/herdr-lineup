//! Lineup command-line scaffold and planned runtime modules.

pub mod config;
pub mod configure;
pub mod daemon;
pub mod doctor;
pub mod font;
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
    /// Ensure one detached daemon is running for this Herdr session.
    Ensure,
    /// Run the daemon in the foreground.
    Daemon,
    /// Stop only this plugin's daemon.
    Stop,
    /// Check local config, Herdr compatibility, and daemon health.
    Doctor,
    /// Planned, opt-in: add sidebar rows to the user's Herdr configuration.
    Configure,
    /// Planned, opt-in: remove only sidebar rows managed by this plugin.
    Unconfigure,
    /// Opt in to installing the bundled icon font and terminal codepoint maps.
    InstallFont,
    /// Remove only the icon font and codepoint maps installed by Lineup.
    UninstallFont,
}

/// Run the command-line scaffold.
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Ensure => daemon::Options::from_env().and_then(|options| daemon::ensure(&options)),
        Command::Daemon => {
            daemon::Options::from_env().and_then(|options| daemon::foreground(&options))
        }
        Command::Stop => daemon::Options::from_env().and_then(|options| daemon::stop(&options)),
        Command::Doctor => daemon::Options::from_env().and_then(|options| doctor::run(&options)),
        Command::Configure | Command::Unconfigure => Err(std::io::Error::other(
            "Planned: this command is not implemented yet. No changes were made.",
        )),
        Command::InstallFont | Command::UninstallFont => {
            font::run(matches!(cli.command, Command::UninstallFont))
                .map(|notes| {
                    for note in notes {
                        println!("{note}");
                    }
                })
                .map_err(|error| std::io::Error::other(format!("font: {error}")))
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
