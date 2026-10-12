//! Lineup command-line scaffold and planned runtime modules.

pub mod config;
pub mod configure;
pub mod daemon;
pub mod doctor;
pub mod font;
pub mod lock;
pub mod model;
pub mod protocol;
pub mod publish;
pub mod quota_codex;
pub mod reader_agy;
pub mod reader_claude;
pub mod reader_codex;
pub mod reader_grok;
pub mod reader_hermes;
pub mod reader_kimi;
pub mod ship_time;
pub mod socket;
pub mod statusline;
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
    /// Capture local usage without changing the existing statusLine display.
    Statusline {
        #[arg(value_enum)]
        agent: statusline::Agent,
        /// Run a shell command with the original stdin and preserve its stdout/status.
        #[arg(long)]
        then: Option<String>,
    },
    /// Ensure one detached daemon is running for this Herdr session.
    Ensure,
    /// Run the daemon in the foreground.
    Daemon,
    /// Stop only this plugin's daemon.
    Stop,
    /// Check local config, Herdr compatibility, and daemon health.
    Doctor,
    /// Opt in to adding sidebar rows to the user's Herdr configuration.
    Configure,
    /// Remove only unmodified sidebar rows managed by this plugin.
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
        Command::Statusline { agent, then } => {
            return ExitCode::from(statusline::run(agent, then.as_deref()));
        }
        Command::Ensure => daemon::Options::from_env().and_then(|options| daemon::ensure(&options)),
        Command::Daemon => {
            daemon::Options::from_env().and_then(|options| daemon::foreground(&options))
        }
        Command::Stop => daemon::Options::from_env().and_then(|options| daemon::stop(&options)),
        Command::Doctor => daemon::Options::from_env().and_then(|options| doctor::run(&options)),
        Command::Configure | Command::Unconfigure => {
            let action = if matches!(cli.command, Command::Configure) {
                configure::Action::Configure
            } else {
                configure::Action::Unconfigure
            };
            match configure::run(action) {
                Ok(configure::Outcome::Unchanged) => {
                    eprintln!("No configuration changes needed.");
                    Ok(())
                }
                Ok(configure::Outcome::Changed { backup }) => {
                    eprintln!(
                        "Configuration updated and reloaded. Backup: {}",
                        backup.display()
                    );
                    Ok(())
                }
                Err(error) => Err(std::io::Error::other(format!(
                    "Configuration action failed: {error}"
                ))),
            }
        }
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
            ExitCode::from(failure_exit_code(&error))
        }
    }
}

pub(crate) fn failure_exit_code(error: &std::io::Error) -> u8 {
    if error
        .get_ref()
        .is_some_and(|source| source.is::<lock::StartupLockContention>())
    {
        daemon::LOCK_CONTENDED_EXIT_CODE
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    #[test]
    fn statusline_cli_accepts_provider_and_chained_command() {
        for provider in ["claude", "agy"] {
            assert!(
                super::Cli::try_parse_from([
                    "herdr-lineup",
                    "statusline",
                    provider,
                    "--then",
                    "cat"
                ])
                .is_ok()
            );
        }
    }
    #[test]
    fn ordinary_socket_and_publisher_would_block_errors_are_not_lock_contention() {
        for message in ["socket read stalled", "publisher write stalled"] {
            let error = std::io::Error::new(std::io::ErrorKind::WouldBlock, message);
            assert_eq!(super::failure_exit_code(&error), 1);
        }
        assert_eq!(
            super::failure_exit_code(&std::io::Error::from_raw_os_error(libc::EAGAIN)),
            1
        );
    }
}
