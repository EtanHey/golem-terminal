use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;

// ── SessionHandle ─────────────────────────────────────────────────────────────

/// A running PTY session.
///
/// Dropping this sends SIGTERM to the child and makes a best-effort reap.
/// Call `.wait()` explicitly for a clean, blocking exit.
pub struct SessionHandle {
    child: Box<dyn portable_pty::Child>,
    /// Receives raw bytes from the child's stdout/stderr as they arrive.
    pub output: mpsc::Receiver<Vec<u8>>,
    input_tx: mpsc::SyncSender<Vec<u8>>,
    // Kept alive so the PTY master fd stays open for the lifetime of the session.
    _master: Box<dyn portable_pty::MasterPty>,
}

impl SessionHandle {
    /// Return the PID of the child process, if available.
    #[cfg(feature = "gui")]
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Write raw bytes to the child's stdin (as if typed at the keyboard).
    pub fn send(&self, bytes: &[u8]) -> Result<()> {
        self.input_tx
            .send(bytes.to_vec())
            .context("session input channel closed — child may have exited")
    }

    /// Move the output receiver out of the handle so it can be given to a
    /// display thread.  After this call, `handle.output` will never yield
    /// further messages; all output arrives on the returned receiver.
    pub fn take_output(&mut self) -> mpsc::Receiver<Vec<u8>> {
        let (_, empty) = mpsc::channel();
        std::mem::replace(&mut self.output, empty)
    }

    /// Send SIGTERM to the child process.
    pub fn kill(&mut self) -> Result<()> {
        if let Some(pid) = self.child.process_id() {
            let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            anyhow::ensure!(
                ret == 0,
                "kill(SIGTERM) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    /// Block until the child exits; return its exit code.
    pub fn wait(&mut self) -> Result<u32> {
        Ok(self.child.wait().context("wait failed")?.exit_code())
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        let _ = self.kill();
        // Non-blocking reap so we don't leave a zombie.
        let _ = self.child.try_wait();
    }
}

// ── spawn ─────────────────────────────────────────────────────────────────────

/// Spawn `cmd` inside a PTY.
///
/// Returns a `SessionHandle` for sending input, reading output, and
/// controlling the child process lifecycle.
pub fn spawn(cmd: Vec<String>) -> Result<SessionHandle> {
    let (cols, rows) = crate::pty::terminal_size();
    spawn_inner(cmd, cols, rows)
}

/// Spawn with an explicit terminal size (used by the GUI to create a larger
/// virtual terminal than the host's real dimensions).
#[cfg(feature = "gui")]
pub fn spawn_sized(cmd: Vec<String>, cols: u16, rows: u16) -> Result<SessionHandle> {
    spawn_inner(cmd, cols, rows)
}

fn spawn_inner(cmd: Vec<String>, cols: u16, rows: u16) -> Result<SessionHandle> {
    anyhow::ensure!(!cmd.is_empty(), "spawn: no command specified");

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
    // Strip env vars that would confuse child processes:
    // - CLAUDECODE: prevents Claude Code from thinking it's nested
    // - SESHAT_TEST_SOCKET: prevents children from binding our control socket
    cmd_builder.env_remove("CLAUDECODE");
    cmd_builder.env_remove("SESHAT_TEST_SOCKET");
    if let Ok(cwd) = std::env::current_dir() {
        cmd_builder.cwd(cwd);
    }

    let child = pair
        .slave
        .spawn_command(cmd_builder)
        .context("spawn_command failed")?;
    drop(pair.slave);

    // Disable PTY echo via the master fd. When the GUI sends input through
    // the PTY, we don't want the terminal driver to echo it back — the GUI
    // manages its own input display. Child programs that need raw mode (like
    // Claude Code) set their own terminal flags on the slave side.
    if let Some(master_fd) = pair.master.as_raw_fd() {
        unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(master_fd, &mut termios) == 0 {
                termios.c_lflag &= !libc::ECHO;
                libc::tcsetattr(master_fd, libc::TCSANOW, &termios);
            }
        }
    }

    let mut master_reader = pair.master.try_clone_reader().context("clone PTY reader")?;
    let mut master_writer = pair.master.take_writer().context("take PTY writer")?;

    // Thread: PTY master → output channel
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match master_reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if output_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Thread: input channel → PTY master
    let (input_tx, input_rx) = mpsc::sync_channel::<Vec<u8>>(8);
    std::thread::spawn(move || {
        for bytes in input_rx {
            if master_writer.write_all(&bytes).is_err() {
                break;
            }
            if master_writer.flush().is_err() {
                break;
            }
        }
    });

    Ok(SessionHandle {
        child,
        output: output_rx,
        input_tx,
        _master: pair.master,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_captures_output_and_exit_code() {
        // Use sh -c so the process lives long enough for the reader thread
        // to capture output.  `cat /dev/null` exits immediately with 0 but
        // gives the PTY time to flush the echo.
        let mut handle = spawn(vec![
            "sh".into(),
            "-c".into(),
            "echo 'hello from session'; cat /dev/null".into(),
        ])
        .unwrap();

        let mut output = Vec::new();
        while let Ok(chunk) = handle.output.recv() {
            output.extend(chunk);
        }

        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("hello from session"),
            "expected output to contain greeting, got: {text:?}"
        );

        let code = handle.wait().unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn session_kill_terminates_child() {
        let mut handle = spawn(vec!["cat".into()]).unwrap();
        handle.kill().expect("kill should succeed");
        let code = handle.wait().unwrap();
        assert_ne!(code, 0, "expected non-zero exit after SIGTERM");
    }

    #[test]
    fn first_output_chunk_is_ready_signal() {
        // Use a long-lived process so the PTY stays open while we read.
        // Plain `echo` exits so fast the reader thread may miss the data.
        let mut handle = spawn(vec![
            "sh".into(),
            "-c".into(),
            "echo ready; exec cat".into(),
        ])
        .unwrap();
        let first = handle.output.recv().expect("first chunk should arrive");
        assert!(!first.is_empty(), "first output chunk must be non-empty");
        assert!(
            String::from_utf8_lossy(&first).contains("ready"),
            "first chunk should contain expected output, got: {:?}",
            String::from_utf8_lossy(&first)
        );
        handle.kill().ok();
    }

    #[test]
    fn take_output_yields_child_output() {
        // Keep child alive with `exec cat` so the PTY channel stays open.
        let mut handle = spawn(vec![
            "sh".into(),
            "-c".into(),
            "echo transferred; exec cat".into(),
        ])
        .unwrap();

        let rx = handle.take_output();
        let mut output = Vec::new();
        while let Ok(chunk) = rx.recv() {
            output.extend(&chunk);
            if String::from_utf8_lossy(&output).contains("transferred") {
                break;
            }
        }

        assert!(
            String::from_utf8_lossy(&output).contains("transferred"),
            "expected 'transferred' in output after take_output()"
        );
        handle.kill().ok();
    }

    #[test]
    fn spawn_empty_command_returns_error() {
        let result = spawn(vec![]);
        assert!(result.is_err(), "spawn with empty command should fail");
    }
}

// ── GUI-specific tests (spawn_sized) ─────────────────────────────────────────

#[cfg(all(test, feature = "gui"))]
mod gui_tests {
    use super::*;
    use std::time::Duration;

    /// Test the spawn_sized → send() → output round-trip directly,
    /// without Iced. This mirrors what the GUI does: spawn with
    /// VT_COLS=120, VT_ROWS=24, then send input via handle.send().
    #[test]
    fn spawn_sized_cat_echo_round_trip() {
        let mut handle = spawn_sized(
            vec!["sh".into(), "-c".into(), "echo ready; exec cat".into()],
            120,
            24,
        )
        .expect("spawn_sized should succeed");

        // Drain output until we see the "ready" signal.
        let mut output = Vec::new();
        loop {
            let chunk = handle
                .output
                .recv_timeout(Duration::from_secs(5))
                .expect("should receive ready signal from child");
            output.extend(&chunk);
            if String::from_utf8_lossy(&output).contains("ready") {
                break;
            }
        }

        // Now send input — cat should echo it back.
        handle.send(b"test123\r").expect("send() should succeed");

        // Collect output until we see "test123" echoed back.
        let mut echo_output = Vec::new();
        loop {
            match handle.output.recv_timeout(Duration::from_secs(5)) {
                Ok(chunk) => {
                    echo_output.extend(&chunk);
                    let text = String::from_utf8_lossy(&echo_output);
                    if text.contains("test123") {
                        break;
                    }
                }
                Err(_) => {
                    let text = String::from_utf8_lossy(&echo_output);
                    panic!("timed out waiting for echo of 'test123'; got so far: {text:?}");
                }
            }
        }

        let text = String::from_utf8_lossy(&echo_output);
        assert!(
            text.contains("test123"),
            "expected echoed input 'test123', got: {text:?}"
        );
        handle.kill().ok();
    }

    /// Test that send() returns Ok when the channel is alive, and verify
    /// the writer thread flushes data to the PTY master.
    #[test]
    fn spawn_sized_send_returns_ok_and_flushes() {
        let mut handle = spawn_sized(
            vec!["sh".into(), "-c".into(), "echo ready; exec cat".into()],
            120,
            24,
        )
        .expect("spawn_sized should succeed");

        // Wait for ready signal.
        let mut output = Vec::new();
        loop {
            let chunk = handle
                .output
                .recv_timeout(Duration::from_secs(5))
                .expect("should receive ready signal");
            output.extend(&chunk);
            if String::from_utf8_lossy(&output).contains("ready") {
                break;
            }
        }

        // send() should return Ok when the child is alive.
        let result = handle.send(b"ping\r");
        assert!(
            result.is_ok(),
            "send() should return Ok, got: {:?}",
            result.err()
        );

        // Verify the data actually made it through (cat echoes it back).
        let mut echo_output = Vec::new();
        loop {
            match handle.output.recv_timeout(Duration::from_secs(5)) {
                Ok(chunk) => {
                    echo_output.extend(&chunk);
                    if String::from_utf8_lossy(&echo_output).contains("ping") {
                        break;
                    }
                }
                Err(_) => {
                    let text = String::from_utf8_lossy(&echo_output);
                    panic!("timed out waiting for echo of 'ping'; got so far: {text:?}");
                }
            }
        }

        // After killing, send() should eventually fail (channel closes).
        handle.kill().ok();
        handle.wait().ok();
        // The writer thread should notice the PTY is gone and exit,
        // which closes the sync_channel. A subsequent send() should fail.
        // Give it a moment for the writer thread to terminate.
        let mut send_failed = false;
        for _ in 0..20 {
            if handle.send(b"after-kill\n").is_err() {
                send_failed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            send_failed,
            "send() should eventually fail after the child is killed"
        );
    }
}
