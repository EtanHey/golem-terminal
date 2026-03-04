// Golem Terminal — Tab-based terminal multiplexer built on Iced
//
// AIDEV-NOTE: Forked from seshat-swarm. Game layer stripped.
// Core: PTY sessions via portable-pty, VT100 parsing, Iced GUI.
// Layout: Tab bar at top, full-width terminal below.
// Split screen: optional 50/50 horizontal split.

use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, text,
};
use iced::{clipboard, keyboard, Length, Subscription, Task};
use std::sync::{Arc, Mutex};

use crate::session::SessionHandle;

// ── Constants ────────────────────────────────────────────────────────────────

// Terminal dimensions
const VT_COLS: u16 = 120;
const VT_ROWS: u16 = 24;
const VT_SCROLLBACK: usize = 1000;

// Font sizes
const FONT_BODY: f32 = 14.0;
const FONT_SMALL: f32 = 12.0;
const FONT_TINY: f32 = 10.0;
const FONT_TAB: f32 = 13.0;

// Spacing
const SPACING_TIGHT: f32 = 4.0;
const SPACING_NORMAL: f32 = 8.0;

// Layout
const TAB_BAR_HEIGHT: f32 = 36.0;
const BOTTOM_BAR_HEIGHT: f32 = 24.0;
const TAB_PADDING: [f32; 2] = [8.0, 12.0]; // vertical, horizontal
const BORDER_RADIUS: f32 = 4.0;
const BORDER_WIDTH: f32 = 1.0;
const RULE_THICKNESS: f32 = 1.0;

// Colors (iTerm-inspired dark theme)
const BG_PRIMARY: iced::Color = iced::Color::from_rgb(0.11, 0.11, 0.14);
const BG_SECONDARY: iced::Color = iced::Color::from_rgb(0.15, 0.15, 0.19);
const BG_TAB_ACTIVE: iced::Color = iced::Color::from_rgb(0.18, 0.18, 0.22);
const BG_TAB_INACTIVE: iced::Color = iced::Color::from_rgb(0.13, 0.13, 0.16);
const TEXT_PRIMARY: iced::Color = iced::Color::from_rgb(0.87, 0.87, 0.87);
const TEXT_SECONDARY: iced::Color = iced::Color::from_rgb(0.55, 0.55, 0.60);
const TEXT_TAB_ACTIVE: iced::Color = iced::Color::from_rgb(0.95, 0.95, 0.95);
const ACCENT_COLOR: iced::Color = iced::Color::from_rgb(0.40, 0.60, 0.95);
const KILL_BUTTON_COLOR: iced::Color = iced::Color::from_rgb(0.9, 0.3, 0.3);
const HOVER_ALPHA: f32 = 0.15;
const DISABLED_ALPHA: f32 = 0.3;

// Status detection
const CLAUDE_CODE_BANNER_PREFIX: &str = "╭─";
const CLAUDE_CODE_STATUS_PREFIX: &str = "⏺";

// Bracketed paste
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

// Byte size labels
const KB: usize = 1024;
const MB: usize = 1024 * 1024;

// Mode badge
const MODE_BADGE_FONT: f32 = 10.0;
const MODE_BADGE_RADIUS: f32 = 3.0;
const MODE_BADGE_PADDING: [f32; 2] = [2.0, 6.0];
const MODE_TEXT_COLOR: iced::Color = iced::Color::WHITE;

// ── Messages ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    // Session lifecycle
    Launch(usize),
    SessionSpawned(usize),
    LaunchFailed(usize),
    Kill(usize),
    OutputChunk { slot: usize, data: Vec<u8> },
    OutputDone(usize),

    // Input
    SendInput(usize, String),
    PtyKeystroke(String),
    PtyPaste,
    PtyPasteResult(Option<String>),

    // Tab management
    SelectTab(usize),
    NewTab,
    CloseTab(usize),
    SwitchTab(i32), // +1 or -1

    // View modes
    ToggleView(usize),
    ToggleSplit, // toggle split screen

    // System
    KeyboardIgnored,
    Quit,
}

// ── Agent Status ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Pending,
    Ready(std::time::Instant),
}

// ── Output View Mode ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputView {
    Filtered,
    Raw,
}

// ── Permission Mode Detection ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum PermissionMode {
    Normal,
    Plan,
    Bash,
}

impl PermissionMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Plan => "Plan",
            Self::Bash => "Bash",
        }
    }

    pub fn color(&self) -> iced::Color {
        match self {
            Self::Normal => iced::Color::from_rgb(0.2, 0.6, 0.3),
            Self::Plan => iced::Color::from_rgb(0.6, 0.5, 0.2),
            Self::Bash => iced::Color::from_rgb(0.7, 0.3, 0.2),
        }
    }
}

fn detect_permission_mode(status: &str) -> Option<PermissionMode> {
    let lower = status.to_lowercase();
    if lower.contains("plan mode") {
        Some(PermissionMode::Plan)
    } else if lower.contains("bash") {
        Some(PermissionMode::Bash)
    } else if lower.contains("normal") || lower.contains("auto") {
        Some(PermissionMode::Normal)
    } else {
        None
    }
}

// ── Agent Slot ───────────────────────────────────────────────────────────────

pub struct AgentSlot {
    pub id: usize,
    pub cmd: Vec<String>,
    pub label: String,
    pub status: AgentStatus,
    pub session: Option<SessionHandle>,
    pub session_mailbox: Arc<Mutex<Option<(SessionHandle, Vec<u8>)>>>,
    pub vt_parser: vt100::Parser,
    pub output_log: Vec<u8>,
    pub output_view: OutputView,
    pub display_cache: String,
    pub raw_log_cache: String,
    content_range: Option<(usize, usize)>,
}

impl AgentSlot {
    pub fn new(id: usize, cmd: Vec<String>, label: String) -> Self {
        Self {
            id,
            cmd,
            label,
            status: AgentStatus::Idle,
            session: None,
            session_mailbox: Arc::new(Mutex::new(None)),
            vt_parser: vt100::Parser::new(VT_ROWS, VT_COLS, VT_SCROLLBACK),
            output_log: Vec::new(),
            output_view: OutputView::Filtered,
            display_cache: String::new(),
            raw_log_cache: String::new(),
            content_range: None,
        }
    }

    pub fn append_output(&mut self, data: &[u8]) {
        self.output_log.extend_from_slice(data);
        self.vt_parser.process(data);
        self.rebuild_caches();
    }

    fn rebuild_caches(&mut self) {
        let screen = self.vt_parser.screen();
        let full = screen.contents();

        // Compute content range — strip Claude Code banner and status bar
        self.content_range = compute_content_range(&full);
        self.display_cache = match self.content_range {
            Some((start, end)) => compute_display_text(screen, start, end),
            None => full,
        };
    }

    pub fn content_text(&self) -> String {
        self.display_cache.clone()
    }

    pub fn status_text(&self) -> String {
        let screen = self.vt_parser.screen();
        let full = screen.contents();
        full.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .to_string()
    }

    fn load_raw_log(&mut self) {
        self.raw_log_cache = String::from_utf8_lossy(&self.output_log).to_string();
    }

    pub fn log_input(&self, _data: &[u8]) {
        // Could log input for debugging, currently a no-op
    }

    pub fn launch(&mut self) -> Task<Message> {
        if self.status != AgentStatus::Idle {
            return Task::none();
        }
        self.status = AgentStatus::Pending;

        let cmd = self.cmd.clone();
        let slot = self.id;
        let mailbox = self.session_mailbox.clone();

        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    match crate::session::spawn(cmd) {
                        Ok(handle) => {
                            *mailbox.lock().unwrap() = Some((handle, vec![]));
                            slot
                        }
                        Err(_) => slot,
                    }
                })
                .await
                .unwrap_or(slot)
            },
            Message::SessionSpawned,
        )
    }

    pub fn session_spawned(&mut self) -> Task<Message> {
        let entry = self.session_mailbox.lock().unwrap().take();
        if let Some((mut handle, initial)) = entry {
            let rx = handle.take_output();
            if !initial.is_empty() {
                self.append_output(&initial);
            }
            self.status = AgentStatus::Ready(std::time::Instant::now());
            self.session = Some(handle);
            let slot = self.id;

            Task::stream(iced::stream::channel(
                32,
                move |mut sender: futures::channel::mpsc::Sender<Message>| async move {
                    let (done_tx, done_rx) = futures::channel::oneshot::channel::<()>();
                    std::thread::spawn(move || {
                        use futures::sink::SinkExt;
                        while let Ok(chunk) = rx.recv() {
                            if futures::executor::block_on(
                                sender.send(Message::OutputChunk { slot, data: chunk }),
                            )
                            .is_err()
                            {
                                break;
                            }
                        }
                        let _ = futures::executor::block_on(
                            sender.send(Message::OutputDone(slot)),
                        );
                        let _ = done_tx.send(());
                    });
                    let _ = done_rx.await;
                },
            ))
        } else {
            self.status = AgentStatus::Idle;
            Task::done(Message::LaunchFailed(self.id))
        }
    }

    pub fn kill(&mut self) {
        if let Some(ref mut session) = self.session {
            let _ = session.kill();
        }
        self.session = None;
        self.status = AgentStatus::Idle;
    }
}

// ── Content Range Detection ──────────────────────────────────────────────────

fn compute_content_range(text: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut start = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with(CLAUDE_CODE_BANNER_PREFIX)
            || line.starts_with("│")
            || line.starts_with("╰")
        {
            start = i + 1;
        } else if !line.trim().is_empty() {
            break;
        }
    }

    let mut end = lines.len();
    for i in (0..lines.len()).rev() {
        let line = lines[i].trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with(CLAUDE_CODE_STATUS_PREFIX) || detect_status_block(line) {
            end = i;
        } else {
            break;
        }
    }

    if start >= end {
        None
    } else {
        Some((start, end))
    }
}

fn detect_status_block(line: &str) -> bool {
    (line.contains("Auto") || line.contains("Plan") || line.contains("Bash"))
        && (line.contains("$") || line.contains("tokens") || line.contains("cost"))
}

fn compute_display_text(screen: &vt100::Screen, start: usize, end: usize) -> String {
    let full = screen.contents();
    let lines: Vec<&str> = full.lines().collect();
    let slice = &lines[start..end.min(lines.len())];

    let mut result = slice.join("\n");

    let (cursor_row, cursor_col) = (
        screen.cursor_position().0 as usize,
        screen.cursor_position().1 as usize,
    );

    if cursor_row >= start && cursor_row < end {
        let adjusted_row = cursor_row - start;
        let mut display_lines: Vec<String> = result.lines().map(|l| l.to_string()).collect();
        if adjusted_row < display_lines.len() {
            let line = &mut display_lines[adjusted_row];
            while line.len() < cursor_col {
                line.push(' ');
            }
            if cursor_col < line.len() {
                let mut chars: Vec<char> = line.chars().collect();
                if cursor_col < chars.len() {
                    chars[cursor_col] = '▋';
                }
                *line = chars.into_iter().collect();
            } else {
                line.push('▋');
            }
        }
        result = display_lines.join("\n");
    }

    result
}

// ── Keyboard → Terminal Sequence ─────────────────────────────────────────────

/// Translate an Iced keyboard event into terminal byte sequence(s).
fn key_to_terminal_seq(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> Option<String> {
    use keyboard::key::Named;

    // Never forward Cmd combos — those belong to the GUI / OS.
    if modifiers.command() {
        return None;
    }

    // Ctrl+letter → control byte (0x01–0x1A).
    if modifiers.control() {
        if let keyboard::Key::Character(c) = key {
            let ch = c.chars().next()?;
            if ch.is_ascii_lowercase() {
                let byte = (ch as u8) - b'a' + 1;
                return Some(String::from(byte as char));
            }
        }
        if let keyboard::Key::Named(Named::Space) = key {
            return Some(String::from('\0'));
        }
    }

    // Named keys → ANSI escape sequences or single bytes.
    if let keyboard::Key::Named(named) = key {
        // Modifier-only keys produce no terminal output.
        match named {
            Named::Shift | Named::Control | Named::Alt | Named::Super
            | Named::CapsLock | Named::NumLock | Named::ScrollLock | Named::Fn
            | Named::Meta => return None,
            _ => {}
        }

        let seq: Option<&str> = match named {
            Named::Space => Some(" "),
            Named::Enter => Some("\r"),
            Named::Backspace => Some("\x7f"),
            Named::Tab if modifiers.shift() => Some("\x1b[Z"),
            Named::Tab => Some("\t"),
            Named::Escape => Some("\x1b"),
            Named::Insert => Some("\x1b[2~"),
            Named::Delete => Some("\x1b[3~"),
            Named::ArrowUp => Some("\x1b[A"),
            Named::ArrowDown => Some("\x1b[B"),
            Named::ArrowRight => Some("\x1b[C"),
            Named::ArrowLeft => Some("\x1b[D"),
            Named::Home => Some("\x1b[H"),
            Named::End => Some("\x1b[F"),
            Named::PageUp => Some("\x1b[5~"),
            Named::PageDown => Some("\x1b[6~"),
            Named::F1 => Some("\x1bOP"),
            Named::F2 => Some("\x1bOQ"),
            Named::F3 => Some("\x1bOR"),
            Named::F4 => Some("\x1bOS"),
            Named::F5 => Some("\x1b[15~"),
            Named::F6 => Some("\x1b[17~"),
            Named::F7 => Some("\x1b[18~"),
            Named::F8 => Some("\x1b[19~"),
            Named::F9 => Some("\x1b[20~"),
            Named::F10 => Some("\x1b[21~"),
            Named::F11 => Some("\x1b[23~"),
            Named::F12 => Some("\x1b[24~"),
            _ => None,
        };
        if let Some(s) = seq {
            return Some(s.into());
        }
    }

    // Printable characters — use the `text` field from winit.
    if let Some(t) = text {
        if !t.is_empty() {
            return if modifiers.alt() {
                Some(format!("\x1b{t}"))
            } else {
                Some(t.to_owned())
            };
        }
    }

    None
}

fn ctrl_split_point(bytes: &[u8]) -> usize {
    let mut split = bytes.len();
    for i in (0..bytes.len()).rev() {
        if bytes[i] >= 0x20 || bytes[i] == b'\t' {
            split = i + 1;
            break;
        }
    }
    split
}

// ── State ────────────────────────────────────────────────────────────────────

pub struct State {
    slots: Vec<AgentSlot>,
    active_tab: usize,
    split_active: bool,
    split_secondary: usize,
    base_cmd: Vec<String>,
    next_slot_id: usize,

    // Asset handles
    kill_handle: iced::widget::image::Handle,
    filtered_handle: iced::widget::image::Handle,
    raw_handle: iced::widget::image::Handle,

    // Test harness
    test_state: Arc<Mutex<crate::test_harness::TestState>>,
}

impl State {
    pub fn new(cmd: Vec<String>) -> Self {
        let slot = AgentSlot::new(0, cmd.clone(), "Tab 1".to_string());

        let test_state = Arc::new(Mutex::new(crate::test_harness::TestState {
            slots: vec![crate::test_harness::SlotState::default()],
            death_cries_enabled: false,
        }));

        Self {
            slots: vec![slot],
            active_tab: 0,
            split_active: false,
            split_secondary: 0,
            base_cmd: cmd,
            next_slot_id: 1,
            kill_handle: iced::widget::image::Handle::from_bytes(
                include_bytes!("../assets/icon_kill.png").to_vec(),
            ),
            filtered_handle: iced::widget::image::Handle::from_bytes(
                include_bytes!("../assets/icon_filtered.png").to_vec(),
            ),
            raw_handle: iced::widget::image::Handle::from_bytes(
                include_bytes!("../assets/icon_raw.png").to_vec(),
            ),
            test_state,
        }
    }

    fn active_slot(&self) -> Option<usize> {
        if self.active_tab < self.slots.len() {
            Some(self.active_tab)
        } else {
            None
        }
    }

    fn sync_test_state(&self) {
        let mut ts = self.test_state.lock().unwrap();
        ts.slots.resize_with(self.slots.len(), Default::default);
        for (i, slot) in self.slots.iter().enumerate() {
            ts.slots[i] = crate::test_harness::SlotState {
                id: slot.id,
                status: match &slot.status {
                    AgentStatus::Idle => "idle".into(),
                    AgentStatus::Pending => "pending".into(),
                    AgentStatus::Ready(_) => "ready".into(),
                },
                output_bytes: slot.output_log.len(),
                content: slot.content_text(),
            };
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Bounds-check slot indices
        let slot_idx = match &message {
            Message::Launch(s)
            | Message::SessionSpawned(s)
            | Message::LaunchFailed(s)
            | Message::Kill(s)
            | Message::OutputDone(s)
            | Message::ToggleView(s)
            | Message::CloseTab(s) => Some(*s),
            Message::OutputChunk { slot, .. } => Some(*slot),
            Message::SendInput(s, _) => Some(*s),
            Message::SelectTab(_)
            | Message::NewTab
            | Message::SwitchTab(_)
            | Message::ToggleSplit
            | Message::PtyKeystroke(_)
            | Message::PtyPaste
            | Message::PtyPasteResult(_)
            | Message::KeyboardIgnored
            | Message::Quit => None,
        };
        if let Some(idx) = slot_idx {
            if idx >= self.slots.len() {
                return Task::none();
            }
        }

        let task = match message {
            Message::Launch(slot) => self.slots[slot].launch(),
            Message::SessionSpawned(slot) => self.slots[slot].session_spawned(),
            Message::LaunchFailed(slot) => {
                self.slots[slot].status = AgentStatus::Idle;
                Task::none()
            }
            Message::Kill(slot) => {
                self.slots[slot].kill();
                Task::none()
            }
            Message::OutputChunk { slot, data } => {
                self.slots[slot].append_output(&data);
                Task::none()
            }
            Message::OutputDone(slot) => {
                self.slots[slot].session = None;
                self.slots[slot].status = AgentStatus::Idle;
                Task::none()
            }
            Message::PtyPaste => clipboard::read().map(Message::PtyPasteResult),
            Message::PtyPasteResult(result) => {
                let Some(slot) = self.active_slot() else {
                    return Task::none();
                };
                if self.slots[slot].session.is_none() {
                    return Task::none();
                }
                let bracketed = self.slots[slot].vt_parser.screen().bracketed_paste();
                match result {
                    Some(text) if bracketed => {
                        let payload =
                            format!("{BRACKETED_PASTE_START}{text}{BRACKETED_PASTE_END}");
                        Task::done(Message::SendInput(slot, payload))
                    }
                    Some(text) => Task::done(Message::SendInput(slot, text)),
                    None if bracketed => {
                        let payload =
                            format!("{BRACKETED_PASTE_START}{BRACKETED_PASTE_END}");
                        Task::done(Message::SendInput(slot, payload))
                    }
                    None => Task::none(),
                }
            }
            Message::SendInput(slot, data) => {
                self.slots[slot].log_input(data.as_bytes());
                if let Some(ref session) = self.slots[slot].session {
                    let bytes = data.as_bytes();
                    let split = ctrl_split_point(bytes);
                    if split > 0 && split < bytes.len() {
                        let _ = session.send(&bytes[..split]);
                        let tail = String::from_utf8_lossy(&bytes[split..]).into_owned();
                        return Task::done(Message::SendInput(slot, tail));
                    }
                    let _ = session.send(bytes);
                }
                Task::none()
            }
            Message::SelectTab(idx) => {
                if idx < self.slots.len() {
                    self.active_tab = idx;
                    if self.slots[idx].status == AgentStatus::Idle {
                        self.slots[idx].launch()
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::NewTab => {
                let id = self.next_slot_id;
                self.next_slot_id += 1;
                let label = format!("Tab {}", id + 1);
                let mut slot = AgentSlot::new(id, self.base_cmd.clone(), label);
                let task = slot.launch();
                self.slots.push(slot);
                self.active_tab = self.slots.len() - 1;

                let mut ts = self.test_state.lock().unwrap();
                ts.slots.push(crate::test_harness::SlotState::default());
                drop(ts);

                task
            }
            Message::CloseTab(idx) => {
                if self.slots.len() <= 1 {
                    return Task::none();
                }
                self.slots[idx].kill();
                self.slots.remove(idx);
                if self.active_tab >= self.slots.len() {
                    self.active_tab = self.slots.len() - 1;
                }
                if self.split_secondary >= self.slots.len() {
                    self.split_secondary = self.slots.len().saturating_sub(1);
                }
                Task::none()
            }
            Message::SwitchTab(delta) => {
                let len = self.slots.len() as i32;
                if len == 0 {
                    return Task::none();
                }
                let current = self.active_tab as i32;
                let new_idx = (current + delta).rem_euclid(len) as usize;
                self.active_tab = new_idx;
                Task::none()
            }
            Message::ToggleView(slot) => {
                let s = &mut self.slots[slot];
                s.output_view = match s.output_view {
                    OutputView::Filtered => {
                        s.load_raw_log();
                        OutputView::Raw
                    }
                    OutputView::Raw => {
                        s.raw_log_cache.clear();
                        OutputView::Filtered
                    }
                };
                Task::none()
            }
            Message::ToggleSplit => {
                self.split_active = !self.split_active;
                if self.split_active && self.slots.len() > 1 {
                    self.split_secondary = if self.active_tab == 0 { 1 } else { 0 };
                }
                Task::none()
            }
            Message::PtyKeystroke(seq) => {
                if let Some(slot) = self.active_slot() {
                    if self.slots[slot].session.is_some() {
                        Task::done(Message::SendInput(slot, seq))
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::KeyboardIgnored => return Task::none(),
            Message::Quit => {
                for slot in &mut self.slots {
                    slot.kill();
                }
                iced::exit()
            }
        };

        self.sync_test_state();
        task
    }

    pub fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().map(|event| match event {
            // ── GUI shortcuts (Cmd-based) ─────────────────────────────
            // Cmd+Alt+Arrow → switch tabs
            keyboard::Event::KeyPressed {
                key, modifiers, ..
            } if modifiers.command() && modifiers.alt() => match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                    Message::SwitchTab(-1)
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                    Message::SwitchTab(1)
                }
                _ => Message::KeyboardIgnored,
            },
            // Cmd+T → new tab
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "t" && modifiers.command() => Message::NewTab,
            // Cmd+V → paste
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "v" && modifiers.command() => Message::PtyPaste,
            // Cmd+D → toggle split
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "d" && modifiers.command() => Message::ToggleSplit,
            // Cmd+Q → quit
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "q" && modifiers.command() => Message::Quit,
            // Cmd+1-9 → select tab
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if modifiers.command() => {
                if let Some(d) = c.chars().next().and_then(|ch| ch.to_digit(10)) {
                    if d >= 1 && d <= 9 {
                        Message::SelectTab((d - 1) as usize)
                    } else {
                        Message::KeyboardIgnored
                    }
                } else {
                    Message::KeyboardIgnored
                }
            }
            // ── PTY pass-through ──────────────────────────────────────
            keyboard::Event::KeyPressed {
                ref key,
                modifiers,
                ref text,
                ..
            } => {
                if let Some(seq) = key_to_terminal_seq(key, modifiers, text.as_deref()) {
                    Message::PtyKeystroke(seq)
                } else {
                    Message::KeyboardIgnored
                }
            }
            _ => Message::KeyboardIgnored,
        })
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        let tab_bar = self.view_tab_bar_strip();

        if self.split_active && self.slots.len() > 1 {
            let left_idx = self.active_tab;
            let right_idx = self.split_secondary;

            let left_panel = self.view_terminal_panel(left_idx, true);
            let right_panel = self.view_terminal_panel(right_idx, false);

            let split = row![left_panel, rule::vertical(RULE_THICKNESS), right_panel]
                .width(Length::Fill)
                .height(Length::Fill);

            column![tab_bar, rule::horizontal(RULE_THICKNESS), split]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            let terminal = if let Some(idx) = self.active_slot() {
                self.view_terminal_panel(idx, true)
            } else {
                container(text("No tabs open").color(TEXT_SECONDARY))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::Alignment::Center)
                    .align_y(iced::Alignment::Center)
                    .into()
            };

            column![tab_bar, rule::horizontal(RULE_THICKNESS), terminal]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }

    // ── View Helpers ─────────────────────────────────────────────────────

    fn view_tab_bar_strip(&self) -> iced::Element<'_, Message> {
        let mut tabs = row![].spacing(SPACING_TIGHT).align_y(iced::Alignment::Center);

        for (i, slot) in self.slots.iter().enumerate() {
            let is_active = i == self.active_tab;

            let status_indicator = match &slot.status {
                AgentStatus::Idle => {
                    text("○").size(FONT_TINY).color(TEXT_SECONDARY)
                }
                AgentStatus::Pending => {
                    text("◌").size(FONT_TINY).color(ACCENT_COLOR)
                }
                AgentStatus::Ready(_) => {
                    text("●")
                        .size(FONT_TINY)
                        .color(iced::Color::from_rgb(0.3, 0.8, 0.4))
                }
            };

            let tab_label = text(&slot.label).size(FONT_TAB).color(
                if is_active {
                    TEXT_TAB_ACTIVE
                } else {
                    TEXT_SECONDARY
                },
            );

            let close_btn = button(text("×").size(FONT_SMALL).color(TEXT_SECONDARY))
                .style(|_theme, _status| iced::widget::button::Style {
                    background: None,
                    ..Default::default()
                })
                .padding([0.0, 4.0])
                .on_press(Message::CloseTab(i));

            let tab_content = row![status_indicator, tab_label, close_btn]
                .spacing(SPACING_TIGHT)
                .align_y(iced::Alignment::Center);

            let bg = if is_active {
                BG_TAB_ACTIVE
            } else {
                BG_TAB_INACTIVE
            };

            let tab_btn = button(tab_content)
                .style(move |_theme, status| {
                    let mut style = iced::widget::button::Style {
                        background: Some(iced::Background::Color(bg)),
                        border: iced::Border {
                            color: if is_active {
                                ACCENT_COLOR
                            } else {
                                iced::Color::TRANSPARENT
                            },
                            width: 0.0,
                            radius: BORDER_RADIUS.into(),
                        },
                        text_color: TEXT_PRIMARY,
                        ..Default::default()
                    };
                    if matches!(status, iced::widget::button::Status::Hovered) && !is_active
                    {
                        style.background =
                            Some(iced::Background::Color(BG_TAB_ACTIVE));
                    }
                    style
                })
                .padding(TAB_PADDING)
                .on_press(Message::SelectTab(i));

            tabs = tabs.push(tab_btn);
        }

        // New tab button
        let new_tab_btn = button(text("+").size(FONT_TAB).color(TEXT_SECONDARY))
            .style(|_theme, status| {
                let mut style = iced::widget::button::Style {
                    background: None,
                    text_color: TEXT_SECONDARY,
                    ..Default::default()
                };
                if matches!(status, iced::widget::button::Status::Hovered) {
                    style.background =
                        Some(iced::Background::Color(BG_TAB_ACTIVE));
                }
                style
            })
            .padding(TAB_PADDING)
            .on_press(Message::NewTab);

        tabs = tabs.push(new_tab_btn);

        // Split toggle button
        let split_label = if self.split_active { "⧉" } else { "◫" };
        let split_btn = button(text(split_label).size(FONT_TAB).color(TEXT_SECONDARY))
            .style(|_theme, status| {
                let mut style = iced::widget::button::Style {
                    background: None,
                    text_color: TEXT_SECONDARY,
                    ..Default::default()
                };
                if matches!(status, iced::widget::button::Status::Hovered) {
                    style.background =
                        Some(iced::Background::Color(BG_TAB_ACTIVE));
                }
                style
            })
            .padding(TAB_PADDING)
            .on_press(Message::ToggleSplit);

        let bar = row![
            tabs,
            iced::widget::Space::new().width(Length::Fill),
            split_btn,
        ]
        .align_y(iced::Alignment::Center)
        .padding([0.0, SPACING_NORMAL]);

        container(bar)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SECONDARY)),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(TAB_BAR_HEIGHT)
            .into()
    }

    fn view_terminal_panel<'a>(
        &'a self,
        idx: usize,
        is_primary: bool,
    ) -> iced::Element<'a, Message> {
        let slot = &self.slots[idx];
        let width = if self.split_active {
            Length::FillPortion(1)
        } else {
            Length::Fill
        };

        let toolbar = self.view_toolbar(idx, slot);
        let output = self.view_output(slot);
        let bottom = self.view_bottom_bar(slot);

        let panel = column![
            toolbar,
            rule::horizontal(RULE_THICKNESS),
            output,
            rule::horizontal(RULE_THICKNESS),
            bottom,
        ]
        .width(width)
        .height(Length::Fill);

        if self.split_active && !is_primary {
            mouse_area(panel)
                .on_press(Message::SelectTab(idx))
                .into()
        } else {
            panel.into()
        }
    }

    fn view_toolbar<'a>(
        &self,
        idx: usize,
        slot: &AgentSlot,
    ) -> iced::Element<'a, Message> {
        let is_filtered = slot.output_view == OutputView::Filtered;

        let filtered_btn = button(
            text("Filtered").size(FONT_SMALL).color(if is_filtered {
                TEXT_TAB_ACTIVE
            } else {
                TEXT_SECONDARY
            }),
        )
        .style(move |_theme, _status| iced::widget::button::Style {
            background: if is_filtered {
                Some(iced::Background::Color(BG_TAB_ACTIVE))
            } else {
                None
            },
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .padding([2.0, 8.0])
        .on_press_maybe((!is_filtered).then_some(Message::ToggleView(idx)));

        let raw_btn = button(
            text("Raw").size(FONT_SMALL).color(if !is_filtered {
                TEXT_TAB_ACTIVE
            } else {
                TEXT_SECONDARY
            }),
        )
        .style(move |_theme, _status| iced::widget::button::Style {
            background: if !is_filtered {
                Some(iced::Background::Color(BG_TAB_ACTIVE))
            } else {
                None
            },
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .padding([2.0, 8.0])
        .on_press_maybe(is_filtered.then_some(Message::ToggleView(idx)));

        let status_label = match &slot.status {
            AgentStatus::Idle => text("—").size(FONT_SMALL).color(TEXT_SECONDARY),
            AgentStatus::Pending => {
                text("Launching…").size(FONT_SMALL).color(ACCENT_COLOR)
            }
            AgentStatus::Ready(_) => text("Running")
                .size(FONT_SMALL)
                .color(iced::Color::from_rgb(0.3, 0.8, 0.4)),
        };

        let kill_btn = button(text("✕").size(FONT_SMALL).color(KILL_BUTTON_COLOR))
            .style(move |_theme, status| {
                let mut style = iced::widget::button::Style {
                    background: None,
                    border: iced::Border {
                        color: KILL_BUTTON_COLOR,
                        width: BORDER_WIDTH,
                        radius: BORDER_RADIUS.into(),
                    },
                    ..Default::default()
                };
                if matches!(status, iced::widget::button::Status::Hovered) {
                    style.background = Some(iced::Background::Color(
                        KILL_BUTTON_COLOR.scale_alpha(HOVER_ALPHA),
                    ));
                }
                if matches!(status, iced::widget::button::Status::Disabled) {
                    style.border.color =
                        KILL_BUTTON_COLOR.scale_alpha(DISABLED_ALPHA);
                }
                style
            })
            .on_press_maybe(
                matches!(slot.status, AgentStatus::Ready(_))
                    .then_some(Message::Kill(idx)),
            );

        let bar = row![
            filtered_btn,
            raw_btn,
            iced::widget::Space::new().width(Length::Fill),
            status_label,
            kill_btn,
        ]
        .spacing(SPACING_NORMAL)
        .align_y(iced::Alignment::Center)
        .padding([SPACING_TIGHT, SPACING_NORMAL]);

        container(bar)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SECONDARY)),
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    }

    fn view_output<'a>(&self, slot: &'a AgentSlot) -> iced::Element<'a, Message> {
        let content: &str = match slot.output_view {
            OutputView::Filtered => &slot.display_cache,
            OutputView::Raw => &slot.raw_log_cache,
        };

        let terminal = scrollable(
            container(
                text(content)
                    .size(FONT_BODY)
                    .color(TEXT_PRIMARY)
                    .font(iced::Font::MONOSPACE),
            )
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_PRIMARY)),
                ..Default::default()
            })
            .padding(SPACING_NORMAL)
            .width(Length::Fill),
        )
        .anchor_bottom()
        .width(Length::Fill)
        .height(Length::Fill);

        container(terminal)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_PRIMARY)),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_bottom_bar<'a>(
        &self,
        slot: &'a AgentSlot,
    ) -> iced::Element<'a, Message> {
        let status = slot.status_text();
        let mode = detect_permission_mode(&status);

        let byte_count = slot.output_log.len();
        let byte_label = if byte_count < KB {
            format!("[{byte_count} bytes]")
        } else if byte_count < MB {
            format!("[{:.1} KB]", byte_count as f64 / KB as f64)
        } else {
            format!("[{:.1} MB]", byte_count as f64 / MB as f64)
        };
        let byte_indicator = text(byte_label).size(FONT_TINY).color(TEXT_SECONDARY);

        let bar = if let Some(pm) = mode {
            let badge = container(
                text(pm.label())
                    .size(MODE_BADGE_FONT)
                    .color(MODE_TEXT_COLOR),
            )
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(pm.color())),
                border: iced::Border {
                    radius: MODE_BADGE_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .padding(MODE_BADGE_PADDING);

            let hint = text("(Shift+Tab)").size(FONT_TINY).color(TEXT_SECONDARY);

            row![
                badge,
                hint,
                iced::widget::Space::new().width(Length::Fill),
                byte_indicator,
            ]
            .spacing(SPACING_NORMAL)
            .align_y(iced::Alignment::Center)
        } else {
            let status_text = text(status).size(FONT_TINY).color(TEXT_SECONDARY);
            row![
                status_text,
                iced::widget::Space::new().width(Length::Fill),
                byte_indicator,
            ]
            .spacing(SPACING_NORMAL)
        };

        container(bar.width(Length::Fill).padding([2.0, SPACING_NORMAL]))
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SECONDARY)),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(BOTTOM_BAR_HEIGHT)
            .into()
    }
}

// ── Run ──────────────────────────────────────────────────────────────────────

pub fn run(cmd: Vec<String>) -> anyhow::Result<()> {
    let icon = load_window_icon();
    let mut win = iced::window::Settings::default();
    win.icon = icon;

    iced::application(
        move || {
            let state = State::new(cmd.clone());
            let harness_task = crate::test_harness::start(state.test_state.clone());
            (state, harness_task)
        },
        State::update,
        State::view,
    )
    .subscription(State::subscription)
    .title("Golem Terminal")
    .window(win)
    .run()
    .map_err(|e| anyhow::anyhow!("iced: {e}"))
}

fn load_window_icon() -> Option<iced::window::Icon> {
    // Use the kill icon as placeholder; replace with proper icon later
    let img = ::image::load_from_memory(include_bytes!("../assets/icon_kill.png"))
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    iced::window::icon::from_rgba(img.into_raw(), w, h).ok()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_one_tab() {
        let state = State::new(vec!["echo".into(), "hi".into()]);
        assert_eq!(state.slots.len(), 1);
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn new_tab_adds_and_selects() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        assert_eq!(state.slots.len(), 2);
        assert_eq!(state.active_tab, 1);
    }

    #[test]
    fn close_tab_doesnt_remove_last() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::CloseTab(0));
        assert_eq!(state.slots.len(), 1);
    }

    #[test]
    fn switch_tab_wraps_around() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::NewTab);
        assert_eq!(state.active_tab, 2);
        let _ = state.update(Message::SwitchTab(1));
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn toggle_split() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        assert!(!state.split_active);
        let _ = state.update(Message::ToggleSplit);
        assert!(state.split_active);
        let _ = state.update(Message::ToggleSplit);
        assert!(!state.split_active);
    }

    #[test]
    fn idle_launch_transitions_to_pending() {
        let mut state = State::new(vec!["echo".into(), "hi".into()]);
        assert_eq!(state.slots[0].status, AgentStatus::Idle);
        let _ = state.update(Message::Launch(0));
        assert_eq!(state.slots[0].status, AgentStatus::Pending);
    }

    #[test]
    fn output_chunk_feeds_vt100_parser() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::OutputChunk {
            slot: 0,
            data: b"\x1b[31mhello\x1b[0m".to_vec(),
        });
        let content = state.slots[0].content_text();
        assert!(content.contains("hello"));
    }
}
