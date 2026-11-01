//! `kigae`: the CLI (SPEC.md §9). Commands land phase by phase; see PLAN.md.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Live rice switcher for KDE Plasma")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the rices found, current one marked
    List,
    /// Print the active rice
    Current,
    /// Switch to a rice
    Apply {
        id: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Put back what the last switch (or the given one) changed
    Undo {
        #[arg(long)]
        list: bool,
        ts: Option<String>,
    },
    /// Copy a rice's [install] assets into $HOME once
    Install {
        id: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove exactly what `install` copied
    Uninstall { id: String },
    /// Check that a rice's parts are installed, without applying
    Doctor { ids: Vec<String> },
    /// Apply each rice in turn and take its preview screenshot
    Shoot { ids: Vec<String> },
    /// Switch login screen and GRUB (one pkexec prompt)
    Deep { id: String },
    /// Run the D-Bus service and overlay
    Daemon,
    /// Ask the daemon to show the overlay (bound to Meta+Shift+T)
    Overlay,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let phase = match cli.command {
        Command::List
        | Command::Current
        | Command::Apply { .. }
        | Command::Undo { .. }
        | Command::Install { .. }
        | Command::Uninstall { .. }
        | Command::Doctor { .. } => 1,
        Command::Daemon | Command::Overlay => 2,
        Command::Shoot { .. } => 3,
        Command::Deep { .. } => 4,
    };
    eprintln!("kigae: not built yet (phase {phase} in PLAN.md)");
    ExitCode::from(2)
}
