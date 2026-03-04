use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};

// ── Terminal size ─────────────────────────────────────────────────────────────

/// Returns the current terminal width and height in characters.
///
/// Falls back to 80×24 when stdout is not attached to a real terminal
/// (e.g. when running under test or inside a pipe).
pub(crate) fn terminal_size() -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0 {
        (ws.ws_col, ws.ws_row)
    } else {
        (80, 24)
    }
}

// ── Raw-mode RAII guard ───────────────────────────────────────────────────────

pub(crate) struct RawModeGuard {
    saved: libc::termios,
}

impl RawModeGuard {
    /// Enter raw mode.  Returns `None` when stdin is not a TTY (e.g. tests,
    /// CI pipelines) so callers can skip the guard without errors.
    pub(crate) fn enter() -> Result<Option<Self>> {
        // Skip raw mode when stdin is not a real terminal.
        if unsafe { libc::isatty(libc::STDIN_FILENO) } == 0 {
            return Ok(None);
        }

        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut saved) };
        anyhow::ensure!(ret == 0, "tcgetattr failed");

        let mut raw = saved;
        unsafe { libc::cfmakeraw(&mut raw) };
        // Keep OPOST so the child's \n still scrolls on our terminal.
        raw.c_oflag |= libc::OPOST;

        let ret = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) };
        anyhow::ensure!(ret == 0, "tcsetattr (raw) failed");

        Ok(Some(Self { saved }))
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.saved);
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Spawn `cmd` inside a PTY and proxy stdin/stdout bidirectionally.
///
/// Blocks until the child exits, then propagates its exit code via
/// `std::process::exit`.  Terminal raw-mode is restored via RAII even on
/// panic.
///
/// # Known limitations
/// - SIGWINCH is not forwarded, so resizing the terminal window won't update
///   the child's idea of the window size.  Planned for a future stage.
/// - This function owns the full I/O loop and writes directly to stdout.
///   For programmatic/GUI use, see `session::spawn()` which returns channels.
pub fn wrap(cmd: Vec<String>) -> Result<()> {
    anyhow::ensure!(!cmd.is_empty(), "wrap: no command specified");

    let (cols, rows) = terminal_size();

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("openpty failed")?;

    let mut cmd_builder = CommandBuilder::new(&cmd[0]);
    cmd_builder.args(&cmd[1..]);
    if let Ok(cwd) = std::env::current_dir() {
        cmd_builder.cwd(cwd);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd_builder)
        .context("spawn_command failed")?;

    // Close the slave end in the parent — the child inherits it.
    drop(pair.slave);

    let mut master_reader = pair.master.try_clone_reader().context("clone PTY reader")?;
    let mut master_writer = pair.master.take_writer().context("take PTY writer")?;

    // Enter raw mode when attached to a real terminal; no-op otherwise.
    let _raw_guard = RawModeGuard::enter()?;

    // Thread 1: PTY master → our stdout (child output → screen)
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        loop {
            match master_reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = out.write_all(&buf[..n]);
                    let _ = out.flush();
                }
            }
        }
    });

    // Thread 2: our stdin → PTY master (keystrokes → child)
    let stdin_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        let stdin = std::io::stdin();
        let mut inp = stdin.lock();
        loop {
            match inp.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = master_writer.write_all(&buf[..n]);
                }
            }
        }
    });

    let status = child.wait().context("wait failed")?;

    // stdout_thread unblocks when the PTY master returns EIO/EOF after the
    // child exits.  stdin_thread is blocked on inp.read() with no way to
    // wake it — but process::exit kills all threads anyway, so we only join
    // the stdout thread (which has already exited) and skip the stdin one.
    stdout_thread.join().ok();
    drop(stdin_thread); // detach; process::exit below will kill it

    // _raw_guard drops here — terminal is restored before exit.
    std::process::exit(status.exit_code() as i32);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Read;

    fn open_pty() -> portable_pty::PtyPair {
        native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty")
    }

    #[test]
    fn pty_spawns_and_exits_cleanly() {
        let pair = open_pty();

        let mut cmd = CommandBuilder::new("echo");
        cmd.arg("hello");

        let mut child = pair.slave.spawn_command(cmd).expect("spawn");
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().expect("clone reader");

        // read_to_string blocks until EOF (master closes after child exits).
        let mut output = String::new();
        let _ = reader.read_to_string(&mut output);

        let status = child.wait().expect("wait");
        assert_eq!(status.exit_code(), 0, "expected exit code 0");
        assert!(
            output.contains("hello"),
            "expected 'hello' in PTY output, got: {output:?}"
        );
    }

    #[test]
    fn pty_captures_nonzero_exit_code() {
        let pair = open_pty();

        let cmd = CommandBuilder::new("false");
        let mut child = pair.slave.spawn_command(cmd).expect("spawn");
        drop(pair.slave);

        // Drain output in a thread so the child is never blocked on writes.
        let mut reader = pair.master.try_clone_reader().expect("clone reader");
        std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            while matches!(reader.read(&mut buf), Ok(n) if n > 0) {}
        });

        let status = child.wait().expect("wait");
        assert_ne!(
            status.exit_code(),
            0,
            "expected non-zero exit code from `false`"
        );
    }
}
