mod access;
mod api;
mod clock;
mod constants;
mod defaults;
mod folders;
mod history;
mod instance;
mod launcher;
mod server;
mod session;
mod settings;
#[cfg(test)]
mod test_support;
mod update;
mod views;
mod watch;

use std::env;
use std::path::{self, PathBuf};
use std::process::ExitCode;

use crate::constants::{NO_RANDOMNESS_MESSAGE, PROGRAM_NAME, START_FAILED_MESSAGE, VERSION_FLAG};
use crate::settings::{Mode, Settings};

/// Arguments are files to add, as the file manager's "Open with" passes them (ACCESS-08).
#[tokio::main]
async fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == VERSION_FLAG) {
        println!("{PROGRAM_NAME} {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let files: Vec<PathBuf> = arguments
        .iter()
        .filter_map(|argument| path::absolute(argument).ok())
        .collect();
    let Ok(settings) = Settings::from_environment(files) else {
        eprintln!("{START_FAILED_MESSAGE} {NO_RANDOMNESS_MESSAGE}");
        return ExitCode::FAILURE;
    };
    if settings.mode == Mode::Desktop && instance::hand_over(&settings.files_to_open).await {
        return ExitCode::SUCCESS;
    }
    if let Err(error) = server::run(settings).await {
        eprintln!("{START_FAILED_MESSAGE} {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
