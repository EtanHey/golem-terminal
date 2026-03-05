// Golem Terminal V2 — Tab-based terminal multiplexer built on Iced + iced_term
//
// V2 uses iced_term (alacritty_terminal backend) for proper terminal rendering
// with colors, cursor, selection, scrollback. Sidebar navigation (Zen browser
// style) replaces top tab bar. pane_grid provides recursive splits.

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, column, container, mouse_area, row, rule, text, Space};
use iced::{keyboard, Length, Subscription, Task};
use std::sync::{Arc, Mutex};

use iced_term::bindings::{Binding, BindingAction, InputKind};
use iced_term::TerminalView;

// ── Constants ────────────────────────────────────────────────────────────────

// Font sizes
const FONT_SMALL: f32 = 12.0;
const FONT_TINY: f32 = 10.0;
const FONT_TAB: f32 = 13.0;
const FONT_GROUP: f32 = 11.0;

// Spacing
const SPACING_TIGHT: f32 = 4.0;
const SPACING_NORMAL: f32 = 8.0;

// Layout
const SIDEBAR_WIDTH: f32 = 200.0;
const BOTTOM_BAR_HEIGHT: f32 = 24.0;
const BORDER_RADIUS: f32 = 4.0;
const RULE_THICKNESS: f32 = 1.0;
const STATUS_DOT_SIZE: f32 = 8.0;
const PANE_SPACING: f32 = 2.0;
const PANE_RESIZE_GRAB_AREA: f32 = 6.0;

// Colors (iTerm-inspired dark theme)
const BG_PRIMARY: iced::Color = iced::Color::from_rgb(0.11, 0.11, 0.14);
const BG_SECONDARY: iced::Color = iced::Color::from_rgb(0.15, 0.15, 0.19);
// Sidebar has alpha < 1.0 so macOS vibrancy shows through
const BG_SIDEBAR: iced::Color = iced::Color {
    r: 0.12,
    g: 0.12,
    b: 0.15,
    a: 0.7,
};
const BG_TAB_ACTIVE: iced::Color = iced::Color::from_rgb(0.18, 0.18, 0.22);
const BG_TAB_HOVER: iced::Color = iced::Color::from_rgb(0.16, 0.16, 0.20);
const TEXT_SECONDARY: iced::Color = iced::Color::from_rgb(0.55, 0.55, 0.60);
const TEXT_TAB_ACTIVE: iced::Color = iced::Color::from_rgb(0.95, 0.95, 0.95);
const ACCENT_COLOR: iced::Color = iced::Color::from_rgb(0.40, 0.60, 0.95);
const STATUS_RUNNING: iced::Color = iced::Color::from_rgb(0.3, 0.8, 0.4);
const STATUS_PENDING: iced::Color = iced::Color::from_rgb(0.9, 0.7, 0.2);
const STATUS_IDLE: iced::Color = iced::Color::from_rgb(0.45, 0.45, 0.50);
const FOCUS_BORDER_COLOR: iced::Color = iced::Color::from_rgb(0.35, 0.55, 0.90);

// ── Slot Status ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SlotStatus {
    Idle,
    Pending,
    Running,
}

// ── Messages ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    // Tab management (sidebar)
    SelectTab(usize),
    NewTab,
    CloseTab(usize),
    SwitchTab(i32),

    // Pane grid
    PaneClicked(pane_grid::Pane),
    PaneDragged(pane_grid::DragEvent),
    PaneResized(pane_grid::ResizeEvent),
    SplitFocused(pane_grid::Axis),
    ClosePane,
    ToggleSplit, // backward compat: toggle between 1-pane and 2-pane

    // Terminal lifecycle
    LaunchSlot(usize),
    KillSlot(usize),
    SendInput(usize, String),

    // iced_term events (forwarded per terminal instance)
    TermEvent(iced_term::Event),

    // Window
    Quit,
    KeyboardIgnored,

    // Sidebar
    ToggleSidebar,

    // Config hot-reload
    ConfigReloaded(crate::config::AppConfig),
}

// ── Agent Slot ───────────────────────────────────────────────────────────────

pub struct AgentSlot {
    pub id: usize,
    pub label: String,
    pub status: SlotStatus,
    pub terminal: Option<iced_term::Terminal>,
    pub term_settings: iced_term::settings::Settings,
}

impl AgentSlot {
    pub fn new(id: usize, cmd: Vec<String>, label: String) -> Self {
        let program = cmd.first().cloned().unwrap_or_else(|| "/bin/bash".into());
        let args: Vec<String> = cmd.iter().skip(1).cloned().collect();

        let term_settings = iced_term::settings::Settings {
            font: iced_term::settings::FontSettings {
                size: 14.0,
                font_type: iced::Font::MONOSPACE,
                ..Default::default()
            },
            theme: iced_term::settings::ThemeSettings::new(Box::new(iterm_color_palette())),
            backend: iced_term::settings::BackendSettings {
                program,
                args,
                ..Default::default()
            },
        };

        Self {
            id,
            label,
            status: SlotStatus::Idle,
            terminal: None,
            term_settings,
        }
    }

    pub fn launch(&mut self) {
        if self.terminal.is_some() {
            return;
        }
        self.status = SlotStatus::Pending;
        match iced_term::Terminal::new(self.id as u64, self.term_settings.clone()) {
            Ok(mut term) => {
                // Register bindings that swallow our Cmd shortcuts so iced_term
                // doesn't forward the letter to the PTY.
                term.handle(iced_term::Command::AddBindings(gui_shortcut_bindings()));
                self.terminal = Some(term);
                self.status = SlotStatus::Running;
            }
            Err(e) => {
                eprintln!("Failed to launch terminal {}: {e}", self.id);
                self.status = SlotStatus::Idle;
            }
        }
    }

    pub fn kill(&mut self) {
        self.terminal = None;
        self.status = SlotStatus::Idle;
    }

    pub fn status_text(&self) -> String {
        match self.status {
            SlotStatus::Idle => "Idle".into(),
            SlotStatus::Pending => "Launching...".into(),
            SlotStatus::Running => "Running".into(),
        }
    }
}

// ── iTerm Color Palette ──────────────────────────────────────────────────────

fn iterm_color_palette() -> iced_term::ColorPalette {
    iced_term::ColorPalette {
        foreground: "#d4d4d4".into(),
        background: "#1e1e1e".into(),
        black: "#000000".into(),
        red: "#cd3131".into(),
        green: "#0dbc79".into(),
        yellow: "#e5e510".into(),
        blue: "#2472c8".into(),
        magenta: "#bc3fbc".into(),
        cyan: "#11a8cd".into(),
        white: "#e5e5e5".into(),
        bright_black: "#666666".into(),
        bright_red: "#f14c4c".into(),
        bright_green: "#23d18b".into(),
        bright_yellow: "#f5f543".into(),
        bright_blue: "#3b8eea".into(),
        bright_magenta: "#d670d6".into(),
        bright_cyan: "#29b8db".into(),
        bright_white: "#e5e5e5".into(),
        bright_foreground: None,
        dim_foreground: "#828482".into(),
        dim_black: "#0f0f0f".into(),
        dim_red: "#712b2b".into(),
        dim_green: "#5f6f3a".into(),
        dim_yellow: "#a17e4d".into(),
        dim_blue: "#456877".into(),
        dim_magenta: "#704d68".into(),
        dim_cyan: "#4d7770".into(),
        dim_white: "#8e8e8e".into(),
    }
}

// ── GUI Shortcut Bindings ─────────────────────────────────────────────────
// These bindings prevent Cmd+letter shortcuts from being forwarded to the PTY
// by iced_term. BindingAction::LinkOpen is a no-op in iced_term's keyboard
// handler (falls to `_ => {}`).

fn gui_shortcut_bindings() -> Vec<(Binding<InputKind>, BindingAction)> {
    use iced::keyboard::Modifiers;

    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

    let swallow = |key: &str, mods: Modifiers| -> (Binding<InputKind>, BindingAction) {
        (
            Binding {
                target: InputKind::Char(key.to_string()),
                modifiers: mods,
                terminal_mode_include: iced_term::TermMode::empty(),
                terminal_mode_exclude: iced_term::TermMode::empty(),
            },
            BindingAction::LinkOpen,
        )
    };

    vec![
        swallow("d", cmd),       // Cmd+D → toggle split
        swallow("d", cmd_shift), // Cmd+Shift+D → split vertical
        swallow("b", cmd),       // Cmd+B → toggle sidebar
        swallow("t", cmd),       // Cmd+T → new tab
        swallow("w", cmd),       // Cmd+W → close pane
        swallow("q", cmd),       // Cmd+Q → quit
        // Cmd+1-9 → select tab
        swallow("1", cmd),
        swallow("2", cmd),
        swallow("3", cmd),
        swallow("4", cmd),
        swallow("5", cmd),
        swallow("6", cmd),
        swallow("7", cmd),
        swallow("8", cmd),
        swallow("9", cmd),
    ]
}

// ── State ────────────────────────────────────────────────────────────────────

pub struct State {
    pub slots: Vec<AgentSlot>,
    pub panes: pane_grid::State<usize>, // each pane stores a slot index
    pub focus: Option<pane_grid::Pane>,
    pub sidebar_visible: bool,
    pub base_cmd: Vec<String>,
    pub next_slot_id: usize,
    pub test_state: Arc<Mutex<crate::test_harness::TestState>>,
    pub config: crate::config::AppConfig,
    #[cfg(target_os = "macos")]
    vibrancy_applied: bool,
}

impl State {
    pub fn new(cmd: Vec<String>) -> Self {
        // Load config (creates default if needed)
        crate::config::ensure_default_config();
        let config = crate::config::load().unwrap_or_default();

        let slot = AgentSlot::new(0, cmd.clone(), "Agent 1".into());
        let (panes, first_pane) = pane_grid::State::new(0_usize); // pane 0 → slot 0

        let test_state = Arc::new(Mutex::new(crate::test_harness::TestState {
            slots: vec![crate::test_harness::SlotState::default()],
            active_tab: 0,
            split_active: false,
            split_secondary: 0,
            death_cries_enabled: false,
        }));

        Self {
            slots: vec![slot],
            panes,
            focus: Some(first_pane),
            sidebar_visible: true,
            base_cmd: cmd,
            next_slot_id: 1,
            test_state,
            config,
            #[cfg(target_os = "macos")]
            vibrancy_applied: false,
        }
    }

    /// The slot index of the focused pane, if any.
    pub fn active_slot_idx(&self) -> Option<usize> {
        self.focus.and_then(|p| self.panes.get(p).copied())
    }

    /// Check if a slot index is currently visible in any pane.
    fn slot_in_pane(&self, slot_idx: usize) -> Option<pane_grid::Pane> {
        self.panes.iter().find_map(|(pane, &idx)| {
            if idx == slot_idx { Some(*pane) } else { None }
        })
    }

    fn sync_test_state(&self) {
        let mut ts = self.test_state.lock().unwrap();
        ts.active_tab = self.active_slot_idx().unwrap_or(0);
        ts.split_active = self.panes.len() > 1;

        // For backward compat, find the "secondary" pane's slot
        if self.panes.len() > 1 {
            if let Some(focus) = self.focus {
                // secondary = first pane that isn't the focused one
                ts.split_secondary = self.panes.iter()
                    .find(|(p, _)| **p != focus)
                    .map(|(_, &idx)| idx)
                    .unwrap_or(0);
            }
        } else {
            ts.split_secondary = 0;
        }

        // Sync slot statuses
        while ts.slots.len() < self.slots.len() {
            ts.slots.push(crate::test_harness::SlotState::default());
        }
        ts.slots.truncate(self.slots.len());

        for (i, slot) in self.slots.iter().enumerate() {
            ts.slots[i].status = match slot.status {
                SlotStatus::Idle => "idle".into(),
                SlotStatus::Pending => "pending".into(),
                SlotStatus::Running => "ready".into(),
            };
        }
    }

    // ── Update ───────────────────────────────────────────────────────────────

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Apply vibrancy on first update (window now exists, we're on main thread)
        // DISABLED: vibrancy applies to entire content view, making terminal grey.
        // Needs per-subview vibrancy (sidebar only) — Phase 2 task.
        #[cfg(target_os = "macos")]
        if !self.vibrancy_applied {
            self.vibrancy_applied = true;
            // if objc2::MainThreadMarker::new().is_some() {
            //     apply_macos_vibrancy();
            // }
        }

        let task = match message {
            Message::LaunchSlot(idx) => {
                if idx < self.slots.len() && self.slots[idx].status == SlotStatus::Idle {
                    self.slots[idx].launch();
                    if let Some(ref term) = self.slots[idx].terminal {
                        return TerminalView::focus(term.widget_id().clone());
                    }
                }
                Task::none()
            }

            Message::KillSlot(idx) => {
                if idx < self.slots.len() {
                    self.slots[idx].kill();
                }
                Task::none()
            }

            Message::SendInput(idx, data) => {
                if let Some(slot) = self.slots.get_mut(idx) {
                    if let Some(ref mut terminal) = slot.terminal {
                        let cmd = iced_term::Command::ProxyToBackend(
                            iced_term::backend::Command::Write(data.into_bytes()),
                        );
                        terminal.handle(cmd);
                    }
                }
                Task::none()
            }

            Message::SelectTab(idx) => {
                if idx < self.slots.len() {
                    // If this slot is already in a pane, focus that pane
                    if let Some(pane) = self.slot_in_pane(idx) {
                        self.focus = Some(pane);
                    } else if let Some(focused) = self.focus {
                        // Otherwise, update the focused pane to show this slot
                        if let Some(slot_ref) = self.panes.get_mut(focused) {
                            *slot_ref = idx;
                        }
                    }
                    // Focus the terminal widget
                    if let Some(ref term) = self.slots[idx].terminal {
                        self.sync_test_state();
                        return TerminalView::focus(term.widget_id().clone());
                    }
                }
                Task::none()
            }

            Message::NewTab => {
                let id = self.next_slot_id;
                self.next_slot_id += 1;
                let label = format!("Agent {}", id + 1);
                let slot = AgentSlot::new(id, self.base_cmd.clone(), label);
                self.slots.push(slot);

                // Update focused pane to show the new slot
                let new_idx = self.slots.len() - 1;
                if let Some(focused) = self.focus {
                    if let Some(slot_ref) = self.panes.get_mut(focused) {
                        *slot_ref = new_idx;
                    }
                }

                let mut ts = self.test_state.lock().unwrap();
                ts.slots.push(crate::test_harness::SlotState::default());
                drop(ts);

                Task::none()
            }

            Message::CloseTab(idx) => {
                if self.slots.len() <= 1 {
                    return Task::none();
                }
                self.slots[idx].kill();
                self.slots.remove(idx);

                // Update all pane references: any pane pointing to idx or above
                // needs adjustment
                let pane_updates: Vec<(pane_grid::Pane, usize)> = self.panes.iter()
                    .map(|(pane, &slot_idx)| {
                        let new_idx = if slot_idx == idx {
                            // This pane pointed to the removed slot — clamp
                            idx.min(self.slots.len() - 1)
                        } else if slot_idx > idx {
                            slot_idx - 1
                        } else {
                            slot_idx
                        };
                        (*pane, new_idx)
                    })
                    .collect();

                for (pane, new_idx) in pane_updates {
                    if let Some(slot_ref) = self.panes.get_mut(pane) {
                        *slot_ref = new_idx;
                    }
                }

                Task::none()
            }

            Message::SwitchTab(delta) => {
                // If multiple panes, cycle focus between panes
                if self.panes.len() > 1 {
                    if let Some(focused) = self.focus {
                        // Try primary direction, then fallback to opposite for wrap
                        let (primary, fallback) = if delta > 0 {
                            (pane_grid::Direction::Right, pane_grid::Direction::Left)
                        } else {
                            (pane_grid::Direction::Left, pane_grid::Direction::Right)
                        };
                        let adjacent = self.panes.adjacent(focused, primary)
                            .or_else(|| self.panes.adjacent(focused, fallback));
                        if let Some(adj) = adjacent {
                            self.focus = Some(adj);
                            if let Some(&slot_idx) = self.panes.get(adj) {
                                if slot_idx < self.slots.len() {
                                    if let Some(ref term) = self.slots[slot_idx].terminal {
                                        self.sync_test_state();
                                        return TerminalView::focus(term.widget_id().clone());
                                    }
                                }
                            }
                        }
                    }
                    return Task::none();
                }
                // Single pane — cycle the slot shown in it
                let len = self.slots.len() as i32;
                if len == 0 {
                    return Task::none();
                }
                let current = self.active_slot_idx().unwrap_or(0) as i32;
                let new_idx = (current + delta).rem_euclid(len) as usize;
                if let Some(focused) = self.focus {
                    if let Some(slot_ref) = self.panes.get_mut(focused) {
                        *slot_ref = new_idx;
                    }
                }
                if let Some(ref term) = self.slots[new_idx].terminal {
                    self.sync_test_state();
                    return TerminalView::focus(term.widget_id().clone());
                }
                Task::none()
            }

            Message::SplitFocused(axis) => {
                if let Some(focused) = self.focus {
                    // Create a new slot for the new pane
                    let id = self.next_slot_id;
                    self.next_slot_id += 1;
                    let label = format!("Agent {}", id + 1);
                    let slot = AgentSlot::new(id, self.base_cmd.clone(), label);
                    self.slots.push(slot);
                    let new_idx = self.slots.len() - 1;

                    let mut ts = self.test_state.lock().unwrap();
                    ts.slots.push(crate::test_harness::SlotState::default());
                    drop(ts);

                    if let Some((new_pane, _)) = self.panes.split(axis, focused, new_idx) {
                        self.focus = Some(new_pane);
                    }
                }
                Task::none()
            }

            Message::ToggleSplit => {
                if self.panes.len() > 1 {
                    // Close all panes except focused
                    while self.panes.len() > 1 {
                        // Find a pane that isn't focused
                        let to_close = self.panes.iter()
                            .find(|(p, _)| Some(**p) != self.focus)
                            .map(|(p, _)| *p);
                        if let Some(pane) = to_close {
                            if let Some((_, sibling)) = self.panes.close(pane) {
                                self.focus = Some(sibling);
                            }
                        } else {
                            break;
                        }
                    }
                    // Focus the remaining pane's terminal
                    if let Some(&slot_idx) = self.focus.and_then(|f| self.panes.get(f)) {
                        if slot_idx < self.slots.len() {
                            if let Some(ref term) = self.slots[slot_idx].terminal {
                                self.sync_test_state();
                                return TerminalView::focus(term.widget_id().clone());
                            }
                        }
                    }
                } else if let Some(focused) = self.focus {
                    // Only 1 pane → split vertically with a second slot
                    let secondary = if self.slots.len() > 1 {
                        // Pick the first slot not currently shown
                        let current = self.active_slot_idx().unwrap_or(0);
                        (0..self.slots.len())
                            .find(|&i| i != current)
                            .unwrap_or(0)
                    } else {
                        // Create a new slot
                        let id = self.next_slot_id;
                        self.next_slot_id += 1;
                        let label = format!("Agent {}", id + 1);
                        let slot = AgentSlot::new(id, self.base_cmd.clone(), label);
                        self.slots.push(slot);
                        let mut ts = self.test_state.lock().unwrap();
                        ts.slots.push(crate::test_harness::SlotState::default());
                        drop(ts);
                        self.slots.len() - 1
                    };
                    if let Some((new_pane, _)) = self.panes.split(
                        pane_grid::Axis::Vertical,
                        focused,
                        secondary,
                    ) {
                        self.focus = Some(new_pane);
                        if let Some(ref term) = self.slots[secondary].terminal {
                            self.sync_test_state();
                            return TerminalView::focus(term.widget_id().clone());
                        }
                    }
                }
                Task::none()
            }

            Message::ClosePane => {
                if let Some(focused) = self.focus {
                    if self.panes.len() > 1 {
                        if let Some((_, sibling)) = self.panes.close(focused) {
                            self.focus = Some(sibling);
                            if let Some(&slot_idx) = self.panes.get(sibling) {
                                if slot_idx < self.slots.len() {
                                    if let Some(ref term) = self.slots[slot_idx].terminal {
                                        self.sync_test_state();
                                        return TerminalView::focus(term.widget_id().clone());
                                    }
                                }
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::PaneClicked(pane) => {
                self.focus = Some(pane);
                if let Some(&slot_idx) = self.panes.get(pane) {
                    if slot_idx < self.slots.len() {
                        // If terminal is running, focus it
                        if let Some(ref term) = self.slots[slot_idx].terminal {
                            self.sync_test_state();
                            return TerminalView::focus(term.widget_id().clone());
                        }
                        // If idle, launch it
                        if self.slots[slot_idx].status == SlotStatus::Idle {
                            self.slots[slot_idx].launch();
                            if let Some(ref term) = self.slots[slot_idx].terminal {
                                self.sync_test_state();
                                return TerminalView::focus(term.widget_id().clone());
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                Task::none()
            }

            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
                Task::none()
            }

            Message::PaneDragged(_) => Task::none(),

            Message::TermEvent(iced_term::Event::BackendCall(id, ref cmd)) => {
                if let Some(slot) = self.slots.iter_mut().find(|s| s.id as u64 == id) {
                    if let Some(ref mut term) = slot.terminal {
                        let action = term.handle(iced_term::Command::ProxyToBackend(cmd.clone()));
                        if action == iced_term::actions::Action::Shutdown {
                            slot.terminal = None;
                            slot.status = SlotStatus::Idle;
                        }
                    }
                }
                Task::none()
            }

            Message::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                // Re-focus current terminal
                if let Some(slot_idx) = self.active_slot_idx() {
                    if slot_idx < self.slots.len() {
                        if let Some(ref term) = self.slots[slot_idx].terminal {
                            self.sync_test_state();
                            return TerminalView::focus(term.widget_id().clone());
                        }
                    }
                }
                Task::none()
            }

            Message::ConfigReloaded(new_config) => {
                eprintln!("[config] reloaded: {} golems, {} groups",
                    new_config.golem.len(), new_config.groups.len());
                self.config = new_config;
                Task::none()
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

    // ── Subscription ─────────────────────────────────────────────────────────

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions: Vec<Subscription<Message>> = vec![];

        // Terminal subscriptions (iced_term handles keyboard input internally)
        for slot in &self.slots {
            if let Some(ref term) = slot.terminal {
                subscriptions.push(term.subscription().map(Message::TermEvent));
            }
        }

        // GUI shortcuts (Cmd-based, not forwarded to terminal)
        subscriptions.push(keyboard::listen().map(|event| match event {
            // Cmd+Alt+Left/Right → SwitchTab (cycles panes or tabs)
            keyboard::Event::KeyPressed {
                key, modifiers, ..
            } if modifiers.command() && modifiers.alt() => match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                    Message::SwitchTab(-1)
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                    Message::SwitchTab(1)
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                    Message::SwitchTab(-1)
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                    Message::SwitchTab(1)
                }
                _ => Message::KeyboardIgnored,
            },
            // Cmd+Shift+D → split vertical
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "d" && modifiers.command() && modifiers.shift() => {
                Message::SplitFocused(pane_grid::Axis::Vertical)
            }
            // Cmd+T → new tab
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "t" && modifiers.command() => Message::NewTab,
            // Cmd+D → toggle split (1 pane ↔ 2 panes side-by-side)
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "d" && modifiers.command() => {
                Message::ToggleSplit
            }
            // Cmd+W → close pane
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "w" && modifiers.command() => Message::ClosePane,
            // Cmd+B → toggle sidebar
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "b" && modifiers.command() => Message::ToggleSidebar,
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
            _ => Message::KeyboardIgnored,
        }));

        // Config file watcher for hot-reload
        subscriptions.push(
            crate::config::watch_config().map(Message::ConfigReloaded),
        );

        Subscription::batch(subscriptions)
    }

    // ── View ─────────────────────────────────────────────────────────────────

    pub fn view(&self) -> iced::Element<'_, Message> {
        let focus = self.focus;

        let pane_grid_widget = PaneGrid::new(&self.panes, |pane_id, &slot_idx, _is_maximized| {
            let is_focused = focus == Some(pane_id);
            let content = self.view_pane_content(slot_idx, is_focused);

            pane_grid::Content::new(content)
                .style(move |_theme| {
                    let border_color = if is_focused {
                        FOCUS_BORDER_COLOR
                    } else {
                        iced::Color::TRANSPARENT
                    };
                    container::Style {
                        border: iced::Border {
                            color: border_color,
                            width: if is_focused { 2.0 } else { 0.0 },
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(PANE_SPACING)
        .on_click(Message::PaneClicked)
        .on_drag(Message::PaneDragged)
        .on_resize(PANE_RESIZE_GRAB_AREA, Message::PaneResized);

        let content_area: iced::Element<'_, Message> = container(pane_grid_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        if self.sidebar_visible {
            let sidebar = self.view_sidebar();
            row![sidebar, rule::vertical(RULE_THICKNESS), content_area]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            content_area
        }
    }

    // ── Pane Content ─────────────────────────────────────────────────────────

    fn view_pane_content(
        &self,
        slot_idx: usize,
        _is_focused: bool,
    ) -> iced::Element<'_, Message> {
        if slot_idx >= self.slots.len() {
            return container(text("Invalid pane").color(TEXT_SECONDARY))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into();
        }

        let slot = &self.slots[slot_idx];

        let terminal_view: iced::Element<'_, Message> = if let Some(ref term) = slot.terminal {
            container(
                TerminalView::show(term).map(Message::TermEvent),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_PRIMARY)),
                ..Default::default()
            })
            .into()
        } else {
            // Idle state — show launch prompt
            let launch_text = column![
                text("Terminal idle").size(FONT_SMALL).color(TEXT_SECONDARY),
                text("Press Enter or click to launch")
                    .size(FONT_TINY)
                    .color(TEXT_SECONDARY),
            ]
            .spacing(SPACING_TIGHT)
            .align_x(iced::Alignment::Center);

            let launch_area = button(
                container(launch_text)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::Alignment::Center)
                    .align_y(iced::Alignment::Center),
            )
            .style(|_theme, _status| iced::widget::button::Style {
                background: Some(iced::Background::Color(BG_PRIMARY)),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Message::LaunchSlot(slot_idx));

            launch_area.into()
        };

        let bottom_bar = self.view_bottom_bar(slot);

        column![terminal_view, rule::horizontal(RULE_THICKNESS), bottom_bar]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ── Sidebar ──────────────────────────────────────────────────────────────

    fn view_sidebar(&self) -> iced::Element<'_, Message> {
        let mut sidebar_content = column![].spacing(2.0).padding([SPACING_NORMAL, 0.0]);

        // Group header: "Agents"
        let group_header = container(
            text("AGENTS")
                .size(FONT_GROUP)
                .color(TEXT_SECONDARY),
        )
        .padding([SPACING_TIGHT, SPACING_NORMAL]);
        sidebar_content = sidebar_content.push(group_header);

        let active_slot = self.active_slot_idx();
        // Collect all slot indices currently visible in panes
        let visible_slots: Vec<usize> = self.panes.iter().map(|(_, idx)| *idx).collect();

        // Tab entries
        for (i, slot) in self.slots.iter().enumerate() {
            let is_active = active_slot == Some(i);
            let is_in_split = !is_active && visible_slots.contains(&i);

            // Status dot
            let dot_color = match slot.status {
                SlotStatus::Running => STATUS_RUNNING,
                SlotStatus::Pending => STATUS_PENDING,
                SlotStatus::Idle => STATUS_IDLE,
            };
            let dot = text("●").size(STATUS_DOT_SIZE).color(dot_color);

            // Label
            let label_color = if is_active || is_in_split {
                TEXT_TAB_ACTIVE
            } else {
                TEXT_SECONDARY
            };
            let label = text(&slot.label).size(FONT_TAB).color(label_color);

            let tab_row = row![dot, label]
                .spacing(SPACING_NORMAL)
                .align_y(iced::Alignment::Center);

            // Background
            let bg = if is_active {
                BG_TAB_ACTIVE
            } else if is_in_split {
                BG_TAB_HOVER
            } else {
                iced::Color::TRANSPARENT
            };

            // Left accent border for active tab
            let border_color = if is_active {
                ACCENT_COLOR
            } else if is_in_split {
                FOCUS_BORDER_COLOR
            } else {
                iced::Color::TRANSPARENT
            };

            let tab_container = container(tab_row)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(bg)),
                    border: iced::Border {
                        color: border_color,
                        width: if is_active || is_in_split {
                            2.0
                        } else {
                            0.0
                        },
                        radius: iced::border::radius(0.0).top_right(BORDER_RADIUS).bottom_right(BORDER_RADIUS),
                    },
                    ..Default::default()
                })
                .padding([SPACING_TIGHT, SPACING_NORMAL])
                .width(Length::Fill);

            // Middle-click to close, regular click to select
            let tab_element: iced::Element<'_, Message> = mouse_area(
                button(tab_container)
                    .style(|_theme, _status| iced::widget::button::Style {
                        background: None,
                        ..Default::default()
                    })
                    .padding(0)
                    .on_press(Message::SelectTab(i)),
            )
            .on_middle_press(Message::CloseTab(i))
            .into();

            sidebar_content = sidebar_content.push(tab_element);
        }

        // Spacer
        sidebar_content = sidebar_content.push(Space::new().width(Length::Fill).height(Length::Fill));

        // New tab button at bottom
        let new_tab_btn = button(
            row![
                text("+").size(FONT_TAB).color(TEXT_SECONDARY),
                text("New Agent").size(FONT_SMALL).color(TEXT_SECONDARY),
            ]
            .spacing(SPACING_NORMAL)
            .align_y(iced::Alignment::Center),
        )
        .style(|_theme, status| {
            let mut style = iced::widget::button::Style {
                background: None,
                ..Default::default()
            };
            if matches!(status, iced::widget::button::Status::Hovered) {
                style.background = Some(iced::Background::Color(BG_TAB_HOVER));
            }
            style
        })
        .padding([SPACING_TIGHT, SPACING_NORMAL])
        .width(Length::Fill)
        .on_press(Message::NewTab);

        sidebar_content = sidebar_content.push(new_tab_btn);

        container(sidebar_content)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SIDEBAR)),
                ..Default::default()
            })
            .width(SIDEBAR_WIDTH)
            .height(Length::Fill)
            .into()
    }

    // ── Bottom Bar ───────────────────────────────────────────────────────────

    fn view_bottom_bar<'a>(&self, slot: &'a AgentSlot) -> iced::Element<'a, Message> {
        let status = slot.status_text();

        let status_dot_color = match slot.status {
            SlotStatus::Running => STATUS_RUNNING,
            SlotStatus::Pending => STATUS_PENDING,
            SlotStatus::Idle => STATUS_IDLE,
        };
        let status_dot = text("●").size(STATUS_DOT_SIZE).color(status_dot_color);
        let status_label = text(status).size(FONT_TINY).color(TEXT_SECONDARY);

        let bar = row![
            status_dot,
            status_label,
            Space::new().width(Length::Fill),
        ]
        .spacing(SPACING_TIGHT)
        .align_y(iced::Alignment::Center);

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

// ── macOS Vibrancy ───────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn apply_macos_vibrancy() {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use std::ptr::NonNull;

    // This must be called from the main thread (Iced's update runs on main)
    let mtm = MainThreadMarker::new()
        .expect("apply_macos_vibrancy must be called from main thread");

    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = (unsafe { app.keyWindow() }) else {
        eprintln!("[vibrancy] no key window found");
        return;
    };

    let Some(content_view) = (unsafe { window.contentView() }) else {
        eprintln!("[vibrancy] no content view");
        return;
    };

    // Wrap the NSView pointer for window-vibrancy
    let ns_view_ptr = Retained::as_ptr(&content_view) as *mut std::ffi::c_void;

    struct NsViewHandle(NonNull<std::ffi::c_void>);

    impl iced::window::raw_window_handle::HasWindowHandle for NsViewHandle {
        fn window_handle(
            &self,
        ) -> Result<iced::window::raw_window_handle::WindowHandle<'_>, iced::window::raw_window_handle::HandleError> {
            let handle = iced::window::raw_window_handle::AppKitWindowHandle::new(self.0);
            Ok(unsafe { iced::window::raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
        }
    }

    if let Some(ptr) = NonNull::new(ns_view_ptr) {
        let handle = NsViewHandle(ptr);
        match window_vibrancy::apply_vibrancy(
            &handle,
            window_vibrancy::NSVisualEffectMaterial::Sidebar,
            Some(window_vibrancy::NSVisualEffectState::Active),
            None,
        ) {
            Ok(()) => eprintln!("[vibrancy] applied Sidebar material"),
            Err(e) => eprintln!("[vibrancy] failed: {e}"),
        }
    }
}

// ── Run ──────────────────────────────────────────────────────────────────────

pub fn run(cmd: Vec<String>) -> anyhow::Result<()> {
    let icon = load_window_icon();
    let mut win = iced::window::Settings::default();
    win.icon = icon;

    #[cfg(target_os = "macos")]
    {
        win.platform_specific.title_hidden = true;
        win.platform_specific.titlebar_transparent = true;
        win.platform_specific.fullsize_content_view = true;
        // win.transparent = true; // TEMPORARILY DISABLED for debugging
    }

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
    .theme(|_: &State| iced::Theme::Dark)
    .title("Golem Terminal")
    .window(win)
    .run()
    .map_err(|e| anyhow::anyhow!("iced: {e}"))
}

fn load_window_icon() -> Option<iced::window::Icon> {
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

    fn active_slot(state: &State) -> usize {
        state.active_slot_idx().unwrap_or(0)
    }

    fn pane_count(state: &State) -> usize {
        state.panes.len()
    }

    #[test]
    fn new_state_has_one_tab() {
        let state = State::new(vec!["echo".into(), "hi".into()]);
        assert_eq!(state.slots.len(), 1);
        assert_eq!(active_slot(&state), 0);
    }

    #[test]
    fn new_tab_adds_and_selects() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        assert_eq!(state.slots.len(), 2);
        // NewTab updates focused pane to show the new slot
        assert_eq!(active_slot(&state), 1);
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
        assert_eq!(active_slot(&state), 2);
        let _ = state.update(Message::SwitchTab(1));
        assert_eq!(active_slot(&state), 0);
    }

    #[test]
    fn switch_tab_in_split_cycles_panes() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::SelectTab(0));
        let _ = state.update(Message::ToggleSplit);
        assert!(pane_count(&state) > 1);
        let before = active_slot(&state);
        // SwitchTab in split mode should cycle focus between panes
        let _ = state.update(Message::SwitchTab(1));
        // Focus should have changed to a different pane (different slot)
        let after = active_slot(&state);
        assert_ne!(before, after, "focus should move to different pane");
    }

    #[test]
    fn toggle_split() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        assert_eq!(pane_count(&state), 1);
        let _ = state.update(Message::ToggleSplit);
        assert!(pane_count(&state) > 1, "toggle should create split");
        let _ = state.update(Message::ToggleSplit);
        assert_eq!(pane_count(&state), 1, "toggle again should collapse");
    }

    #[test]
    fn select_tab_no_auto_launch() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        assert_eq!(state.slots[1].status, SlotStatus::Idle);
        let _ = state.update(Message::SelectTab(1));
        // SelectTab should NOT auto-launch — must stay idle
        assert_eq!(state.slots[1].status, SlotStatus::Idle);
    }

    #[test]
    fn switch_tab_wraps_backward() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::SelectTab(0));
        assert_eq!(active_slot(&state), 0);
        let _ = state.update(Message::SwitchTab(-1));
        assert_eq!(active_slot(&state), 1, "SwitchTab(-1) from 0 should wrap to last");
    }

    #[test]
    fn close_tab_clamps_active_tab() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::SelectTab(1));
        assert_eq!(active_slot(&state), 1);
        let _ = state.update(Message::CloseTab(1));
        assert!(
            active_slot(&state) < state.slots.len(),
            "active_tab must clamp after closing last tab"
        );
    }

    #[test]
    fn select_tab_changes_active() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::NewTab);
        assert_eq!(active_slot(&state), 2);
        let _ = state.update(Message::SelectTab(0));
        assert_eq!(active_slot(&state), 0);
    }

    #[test]
    fn split_focused_creates_new_pane() {
        let mut state = State::new(vec!["echo".into()]);
        assert_eq!(pane_count(&state), 1);
        let _ = state.update(Message::SplitFocused(pane_grid::Axis::Horizontal));
        assert_eq!(pane_count(&state), 2);
        assert_eq!(state.slots.len(), 2, "split should create new slot");
    }

    #[test]
    fn close_pane_merges() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::SplitFocused(pane_grid::Axis::Horizontal));
        assert_eq!(pane_count(&state), 2);
        let _ = state.update(Message::ClosePane);
        assert_eq!(pane_count(&state), 1);
    }

    #[test]
    fn close_pane_noop_when_single() {
        let mut state = State::new(vec!["echo".into()]);
        assert_eq!(pane_count(&state), 1);
        let _ = state.update(Message::ClosePane);
        assert_eq!(pane_count(&state), 1, "should not close the last pane");
    }

    #[test]
    fn pane_clicked_sets_focus() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::SplitFocused(pane_grid::Axis::Horizontal));
        // Get the non-focused pane
        let other = state.panes.iter()
            .find(|(p, _)| Some(**p) != state.focus)
            .map(|(p, _)| *p)
            .unwrap();
        let _ = state.update(Message::PaneClicked(other));
        assert_eq!(state.focus, Some(other));
    }

    #[test]
    fn sidebar_toggles() {
        let mut state = State::new(vec!["echo".into()]);
        assert!(state.sidebar_visible);
        let _ = state.update(Message::ToggleSidebar);
        assert!(!state.sidebar_visible);
        let _ = state.update(Message::ToggleSidebar);
        assert!(state.sidebar_visible);
    }

    #[test]
    fn new_tab_is_idle() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        // New tabs should be idle, not auto-launched
        assert_eq!(state.slots[1].status, SlotStatus::Idle);
    }
}
