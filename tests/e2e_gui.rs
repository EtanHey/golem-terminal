//! E2E test for the GUI via the UDS test harness.
//!
//! Requires `--features gui` and a display context (macOS/X11/Wayland).
//! The test launches the binary, connects to the control socket, and drives
//! the GUI through its state machine.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU32, Ordering};

static TEST_ID: AtomicU32 = AtomicU32::new(0);

fn unique_socket_path(label: &str) -> String {
    let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}/golem-test-{label}-{}-{id}.sock",
        std::env::temp_dir().display(),
        std::process::id(),
    )
}

fn connect_uds(path: &str) -> UnixStream {
    for _ in 0..100 {
        if let Ok(stream) = UnixStream::connect(path) {
            return stream;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("could not connect to test socket at {path} within 5s");
}

fn fire(writer: &mut impl Write, cmd: &str) {
    writer.write_all(cmd.as_bytes()).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
}

fn query(writer: &mut impl Write, reader: &mut BufReader<UnixStream>, cmd: &str) -> String {
    writer.write_all(cmd.as_bytes()).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line
}

fn get_status(writer: &mut UnixStream, reader: &mut BufReader<UnixStream>, slot: usize) -> String {
    let cmd = format!(r#"{{"cmd":"status","slot":{slot}}}"#);
    let resp = query(writer, reader, &cmd);
    resp.trim()
        .trim_start_matches(r#"{"status":""#)
        .trim_end_matches(r#""}"#)
        .to_string()
}

fn wait_for_status(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    slot: usize,
    target: &str,
) {
    for _ in 0..200 {
        let status = get_status(writer, reader, slot);
        if status == target {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("slot {slot} status never reached '{target}'");
}

fn get_content(writer: &mut UnixStream, reader: &mut BufReader<UnixStream>, slot: usize) -> String {
    let cmd = format!(r#"{{"cmd":"content","slot":{slot}}}"#);
    let resp = query(writer, reader, &cmd);
    let hex = resp
        .trim()
        .trim_start_matches(r#"{"content":""#)
        .trim_end_matches(r#""}"#)
        .to_string();
    String::from_utf8_lossy(&hex_decode(&hex)).to_string()
}

fn wait_for_content_contains(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    slot: usize,
    target: &str,
) {
    for _ in 0..200 {
        let content = get_content(writer, reader, slot);
        if content.contains(target) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let content = get_content(writer, reader, slot);
    panic!("slot {slot} content never contained '{target}', got: {content:?}");
}

fn get_slot_count(writer: &mut UnixStream, reader: &mut BufReader<UnixStream>) -> usize {
    let resp = query(writer, reader, r#"{"cmd":"slot_count"}"#);
    let trimmed = resp.trim();
    trimmed
        .trim_start_matches(r#"{"slot_count":"#)
        .trim_end_matches('}')
        .parse()
        .unwrap_or(0)
}

// ── GuiProcess ──────────────────────────────────────────────────────────

struct GuiProcess {
    child: Child,
    socket_path: String,
}

impl GuiProcess {
    fn spawn(cmd: &[&str], socket_path: &str) -> Self {
        let manifest = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        let status = Command::new(env!("CARGO"))
            .args(["build", "--features", "gui", "--manifest-path", &manifest])
            .status()
            .expect("cargo build failed");
        assert!(status.success(), "cargo build --features gui failed");

        let binary = format!(
            "{}/debug/golem-terminal",
            std::env::var("CARGO_TARGET_DIR")
                .unwrap_or_else(|_| format!("{}/target", env!("CARGO_MANIFEST_DIR")))
        );

        let mut args = vec!["ui", "--"];
        args.extend_from_slice(cmd);

        let child = Command::new(&binary)
            .args(&args)
            .env("SESHAT_TEST_SOCKET", socket_path)
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {binary}: {e}"));

        Self {
            child,
            socket_path: socket_path.to_string(),
        }
    }
}

impl Drop for GuiProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn quit_closes_app() {
    let socket_path = unique_socket_path("quit");
    let mut proc = GuiProcess::spawn(&["sh", "-c", "echo ready; exec cat"], &socket_path);

    let stream = connect_uds(&socket_path);
    let mut writer = stream.try_clone().unwrap();

    fire(&mut writer, r#"{"cmd":"quit"}"#);

    let status = proc.child.wait().expect("wait failed");
    assert!(status.success(), "expected clean exit, got {status:?}");

    let _ = std::fs::remove_file(&proc.socket_path);
    std::mem::forget(proc);
}

#[test]
fn launch_echo_and_verify_output() {
    let socket_path = unique_socket_path("echo");
    let _proc = GuiProcess::spawn(&["sh", "-c", "echo hello; exec cat"], &socket_path);

    let stream = connect_uds(&socket_path);
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let status = get_status(&mut writer, &mut reader, 0);
    assert_eq!(status, "idle", "expected idle on start, got {status}");

    fire(&mut writer, r#"{"cmd":"launch"}"#);
    wait_for_status(&mut writer, &mut reader, 0, "ready");

    wait_for_content_contains(&mut writer, &mut reader, 0, "hello");

    fire(&mut writer, r#"{"cmd":"kill"}"#);
    wait_for_status(&mut writer, &mut reader, 0, "idle");
}

#[test]
fn launch_and_kill_cat() {
    let socket_path = unique_socket_path("kill");
    let _proc = GuiProcess::spawn(&["sh", "-c", "echo ready; exec cat"], &socket_path);

    let stream = connect_uds(&socket_path);
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    fire(&mut writer, r#"{"cmd":"launch"}"#);
    wait_for_status(&mut writer, &mut reader, 0, "ready");

    fire(&mut writer, r#"{"cmd":"kill"}"#);
    wait_for_status(&mut writer, &mut reader, 0, "idle");
}

#[test]
fn cat_echo_round_trip() {
    let socket_path = unique_socket_path("cat");
    let _proc = GuiProcess::spawn(&["sh", "-c", "echo ready; exec cat"], &socket_path);

    let stream = connect_uds(&socket_path);
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    fire(&mut writer, r#"{"cmd":"launch"}"#);
    wait_for_status(&mut writer, &mut reader, 0, "ready");

    fire(
        &mut writer,
        r#"{"cmd":"send_input","data":"hello from test\r"}"#,
    );

    wait_for_content_contains(&mut writer, &mut reader, 0, "hello from test");

    fire(&mut writer, r#"{"cmd":"kill"}"#);
    wait_for_status(&mut writer, &mut reader, 0, "idle");
}

#[test]
fn new_tab_and_close_tab() {
    let socket_path = unique_socket_path("tabs");
    let _proc = GuiProcess::spawn(&["sh", "-c", "echo ready; exec cat"], &socket_path);

    let stream = connect_uds(&socket_path);
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    // Start with 1 tab
    let initial = get_slot_count(&mut writer, &mut reader);
    assert_eq!(initial, 1, "expected 1 initial tab");

    // Create a new tab
    fire(&mut writer, r#"{"cmd":"new_tab"}"#);

    // Wait for tab count to increase
    for _ in 0..200 {
        if get_slot_count(&mut writer, &mut reader) > initial {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let after_new = get_slot_count(&mut writer, &mut reader);
    assert_eq!(after_new, 2, "expected 2 tabs after new_tab");

    // Wait for new tab to be ready
    wait_for_status(&mut writer, &mut reader, 1, "ready");

    // Kill the new tab's process
    fire(&mut writer, r#"{"cmd":"kill","slot":1}"#);
    wait_for_status(&mut writer, &mut reader, 1, "idle");

    // Close the tab
    fire(&mut writer, r#"{"cmd":"close_tab","slot":1}"#);

    for _ in 0..200 {
        if get_slot_count(&mut writer, &mut reader) < after_new {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let after_close = get_slot_count(&mut writer, &mut reader);
    assert_eq!(after_close, 1, "expected 1 tab after close");
}

fn hex_decode(hex: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.chars();
    while let (Some(a), Some(b)) = (chars.next(), chars.next()) {
        let byte = u8::from_str_radix(&format!("{a}{b}"), 16).unwrap_or(0);
        bytes.push(byte);
    }
    bytes
}
