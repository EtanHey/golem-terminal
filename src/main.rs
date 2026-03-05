mod config;
mod pty;
mod session;
#[cfg(feature = "gui")]
mod test_harness;
#[cfg(feature = "gui")]
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "golem-terminal", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Wrap a command in a PTY and proxy it transparently (interactive).
    Wrap {
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },

    /// Spawn a command, wait for first output (ready signal), then proxy interactively.
    Run {
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },

    /// Launch the Iced GUI for managing terminal tabs.
    #[cfg(feature = "gui")]
    Ui {
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Wrap { cmd } => pty::wrap(cmd),
        Commands::Run { cmd } => run(cmd),
        #[cfg(feature = "gui")]
        Commands::Ui { cmd } => {
            let cmd = if cmd.is_empty() {
                // Use shell from config (defaults to /bin/zsh -l)
                let cfg = config::load().unwrap_or_default();
                let mut shell_cmd = vec![cfg.shell.program];
                shell_cmd.extend(cfg.shell.args);
                shell_cmd            } else {
                cmd
            };
            ui::run(cmd)
        }
    }
}

// ── run ───────────────────────────────────────────────────────────────────────

fn run(cmd: Vec<String>) -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let mut session = session::spawn(cmd)?;

    let first = session
        .output
        .recv()
        .map_err(|_| anyhow::anyhow!("child exited before producing any output"))?;

    let _raw_guard = pty::RawModeGuard::enter()?;

    {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        out.write_all(&first)?;
        out.flush()?;
    }

    let output = session.take_output();
    let stdout_thread = std::thread::spawn(move || {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        while let Ok(chunk) = output.recv() {
            let _ = out.write_all(&chunk);
            let _ = out.flush();
        }
    });

    let stdin = std::io::stdin();
    let mut inp = stdin.lock();
    let mut buf = [0u8; 256];
    loop {
        match inp.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if session.send(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }

    drop(inp);
    stdout_thread.join().ok();
    let code = session.wait()?;
    std::process::exit(code as i32);
}
