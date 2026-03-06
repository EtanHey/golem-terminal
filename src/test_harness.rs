//! UDS-based test control channel for E2E GUI tests.
//!
//! When `SESHAT_TEST_SOCKET` is set, the app opens a Unix domain socket and
//! accepts newline-delimited JSON commands. All commands default to slot 0.
//!
//!   {"cmd":"launch"}               or  {"cmd":"launch","slot":1}
//!   {"cmd":"kill"}                  or  {"cmd":"kill","slot":1}
//!   {"cmd":"status"}               or  {"cmd":"status","slot":1}
//!   {"cmd":"output"}               or  {"cmd":"output","slot":1}
//!   {"cmd":"content"}              or  {"cmd":"content","slot":1}
//!   {"cmd":"send_input","data":"x"} or  {"cmd":"send_input","data":"x","slot":1}
//!   {"cmd":"new_tab"}
//!   {"cmd":"close_tab","slot":1}
//!   {"cmd":"select_tab","slot":1}
//!   {"cmd":"toggle_split"}
//!   {"cmd":"slot_count"}
//!   {"cmd":"active_tab"}
//!   {"cmd":"split_status"}
//!   {"cmd":"quit"}

use crate::ui::Message;
use iced::Task;
use serde::Deserialize;
use std::io::{BufRead, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct SlotState {
    pub id: usize,
    pub status: String,
    pub output_bytes: usize,
    pub content: String,
    pub raw_output: Vec<u8>,
}

pub struct TestState {
    pub slots: Vec<SlotState>,
    pub active_tab: usize,
    pub split_active: bool,
    pub split_secondary: usize,
    pub death_cries_enabled: bool, // kept for compat, unused
}

#[derive(Deserialize)]
struct Command {
    cmd: String,
    #[serde(default)]
    slot: usize,
    #[serde(default)]
    data: Option<String>,
}

/// Start the test harness if SESHAT_TEST_SOCKET is set.
pub fn start(shared: Arc<Mutex<TestState>>) -> Task<Message> {
    let path = match std::env::var("SESHAT_TEST_SOCKET") {
        Ok(p) if !p.is_empty() => p,
        _ => return Task::none(),
    };

    Task::stream(iced::stream::channel(
        32,
        move |mut sender: futures::channel::mpsc::Sender<Message>| async move {
            let (done_tx, done_rx) = futures::channel::oneshot::channel::<()>();
            std::thread::spawn(move || {
                use futures::sink::SinkExt;

                let _ = std::fs::remove_file(&path);
                let listener = match UnixListener::bind(&path) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("[test-harness] failed to bind {path}: {e}");
                        return;
                    }
                };

                for stream_result in listener.incoming() {
                    let Ok(stream) = stream_result else { break };

                    let Ok(cloned) = stream.try_clone() else {
                        eprintln!("[test-harness] stream try_clone failed");
                        continue;
                    };
                    let reader = std::io::BufReader::new(cloned);
                    let mut writer = stream;

                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }

                        let parsed: Command = match serde_json::from_str(&line) {
                            Ok(c) => c,
                            Err(e) => {
                                reply(&mut writer, &error_reply(&e.to_string()));
                                continue;
                            }
                        };

                        // Commands that don't require a valid slot index.
                        let slot_free_cmd = matches!(
                            parsed.cmd.as_str(),
                            "quit"
                                | "slot_count"
                                | "new_tab"
                                | "toggle_split"
                                | "active_tab"
                                | "split_status"
                        );
                        if !slot_free_cmd {
                            let slot_count = shared.lock().unwrap().slots.len();
                            if parsed.slot >= slot_count {
                                reply(
                                    &mut writer,
                                    &error_reply(&format!(
                                        "slot {} out of range (max {})",
                                        parsed.slot,
                                        slot_count.saturating_sub(1)
                                    )),
                                );
                                continue;
                            }
                        }

                        match parsed.cmd.as_str() {
                            "quit" => {
                                let _ = futures::executor::block_on(sender.send(Message::Quit));
                                return;
                            }
                            "launch" => {
                                let _ = futures::executor::block_on(
                                    sender.send(Message::LaunchSlot(parsed.slot)),
                                );
                            }
                            "kill" => {
                                let _ = futures::executor::block_on(
                                    sender.send(Message::KillSlot(parsed.slot)),
                                );
                            }
                            "send_input" => {
                                if let Some(data) = parsed.data {
                                    let _ = futures::executor::block_on(
                                        sender.send(Message::SendInput(parsed.slot, data)),
                                    );
                                }
                            }
                            "status" => {
                                let state = shared.lock().unwrap();
                                let status_str = &state.slots[parsed.slot].status;
                                reply(&mut writer, &format!("{{\"status\":\"{status_str}\"}}\n"));
                            }
                            "content" => {
                                let hex = {
                                    let state = shared.lock().unwrap();
                                    hex_encode(state.slots[parsed.slot].content.as_bytes())
                                };
                                reply(&mut writer, &format!("{{\"content\":\"{hex}\"}}\n"));
                            }
                            "output" => {
                                let state = shared.lock().unwrap();
                                let hex = hex_encode(&state.slots[parsed.slot].raw_output);
                                reply(&mut writer, &format!("{{\"output\":\"{hex}\"}}\n"));
                            }
                            "new_tab" => {
                                let new_slot = shared.lock().unwrap().slots.len();
                                let _ = futures::executor::block_on(sender.send(Message::NewTab));
                                let _ = futures::executor::block_on(
                                    sender.send(Message::LaunchSlot(new_slot)),
                                );
                            }
                            "close_tab" => {
                                let _ = futures::executor::block_on(
                                    sender.send(Message::CloseTab(parsed.slot)),
                                );
                            }
                            "select_tab" => {
                                let _ = futures::executor::block_on(
                                    sender.send(Message::SelectTab(parsed.slot)),
                                );
                            }
                            "toggle_split" => {
                                let _ =
                                    futures::executor::block_on(sender.send(Message::ToggleSplit));
                            }
                            "slot_count" => {
                                let count = shared.lock().unwrap().slots.len();
                                reply(&mut writer, &format!("{{\"slot_count\":{count}}}\n"));
                            }
                            "active_tab" => {
                                let state = shared.lock().unwrap();
                                let tab = state.active_tab;
                                reply(&mut writer, &format!("{{\"active_tab\":{tab}}}\n"));
                            }
                            "split_status" => {
                                let state = shared.lock().unwrap();
                                let active = state.split_active;
                                let secondary = state.split_secondary;
                                reply(
                                    &mut writer,
                                    &format!("{{\"split_active\":{active},\"split_secondary\":{secondary}}}\n"),
                                );
                            }
                            other => {
                                reply(
                                    &mut writer,
                                    &error_reply(&format!("unknown command: {other}")),
                                );
                            }
                        }
                    }
                }

                let _ = done_tx.send(());
            });

            let _ = done_rx.await;
        },
    ))
}

fn reply(writer: &mut impl Write, resp: &str) {
    if let Err(e) = writer
        .write_all(resp.as_bytes())
        .and_then(|_| writer.flush())
    {
        eprintln!("[test-harness] reply failed: {e}");
    }
}

fn error_reply(msg: &str) -> String {
    format!("{{\"error\":\"{}\"}}\n", msg.replace('"', "\\\""))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::hex_encode;

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn hex_encode_known_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn hex_encode_all_zeros() {
        assert_eq!(hex_encode(&[0x00, 0x00]), "0000");
    }
}
