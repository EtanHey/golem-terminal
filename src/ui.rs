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

use crate::agent_state;
use crate::config::{parse_hex_color, parse_hex_color_alpha};

// ── Constants (non-themed, not in config) ───────────────────────────────────

// Spacing (small enough to not warrant config)
const SPACING_TIGHT: f32 = 4.0;
const SPACING_NORMAL: f32 = 8.0;

// Layout (fixed structural values)
const BORDER_RADIUS: f32 = 4.0;
const RULE_THICKNESS: f32 = 1.0;
const STATUS_DOT_SIZE: f32 = 8.0;
const PANE_RESIZE_GRAB_AREA: f32 = 6.0;

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
    ToggleGroup(String),
    LaunchGolem(String),

    // Agent state polling
    AgentStateTick,

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
    pub fn new(
        id: usize,
        cmd: Vec<String>,
        label: String,
        config: &crate::config::AppConfig,
    ) -> Self {
        let program = cmd
            .first()
            .cloned()
            .unwrap_or_else(|| config.shell.program.clone());
        let args: Vec<String> = if cmd.len() > 1 {
            cmd.iter().skip(1).cloned().collect()
        } else if cmd.is_empty() {
            config.shell.args.clone()
        } else {
            // cmd has exactly 1 element (the program), no extra args
            vec![]
        };

        let term_settings = iced_term::settings::Settings {
            font: iced_term::settings::FontSettings {
                size: config.ui.font.terminal,
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
    pub collapsed_groups: std::collections::HashMap<String, bool>,
    pub agent_states: std::collections::HashMap<String, agent_state::AgentExternalState>,
    agent_state_dir: std::path::PathBuf,
    #[cfg(target_os = "macos")]
    vibrancy_applied: bool,
}

impl State {
    pub fn new(cmd: Vec<String>) -> Self {
        // Load config (creates default if needed)
        crate::config::ensure_default_config();
        let config = crate::config::load().unwrap_or_default();

        let slot = AgentSlot::new(0, cmd.clone(), "Agent 1".into(), &config);
        let (panes, first_pane) = pane_grid::State::new(0_usize); // pane 0 → slot 0

        let test_state = Arc::new(Mutex::new(crate::test_harness::TestState {
            slots: vec![crate::test_harness::SlotState::default()],
            active_tab: 0,
            split_active: false,
            split_secondary: 0,
            death_cries_enabled: false,
        }));

        let state = Self {
            slots: vec![slot],
            panes,
            focus: Some(first_pane),
            sidebar_visible: true,
            base_cmd: cmd,
            next_slot_id: 1,
            test_state,
            config,
            collapsed_groups: std::collections::HashMap::new(),
            agent_states: std::collections::HashMap::new(),
            agent_state_dir: agent_state::state_dir(),
            #[cfg(target_os = "macos")]
            vibrancy_applied: false,
        };

        state.sync_test_state();
        state
    }

    /// The slot index of the focused pane, if any.
    pub fn active_slot_idx(&self) -> Option<usize> {
        self.focus.and_then(|p| self.panes.get(p).copied())
    }

    /// Check if a slot index is currently visible in any pane.
    fn slot_in_pane(&self, slot_idx: usize) -> Option<pane_grid::Pane> {
        self.panes.iter().find_map(
            |(pane, &idx)| {
                if idx == slot_idx {
                    Some(*pane)
                } else {
                    None
                }
            },
        )
    }

    /// Returns slot indices that are not tied to any golem preset.
    fn ad_hoc_slots(&self) -> Vec<(usize, &AgentSlot)> {
        let golem_names: std::collections::HashSet<&str> =
            self.config.golem.iter().map(|g| g.name.as_str()).collect();
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| !golem_names.contains(s.label.as_str()))
            .collect()
    }

    /// Returns group names in display order: orchestrators, workers, tools, then
    /// any custom groups alphabetically, then "OTHER" if ungrouped golems exist.
    pub fn ordered_groups(&self) -> Vec<String> {
        let mut result = Vec::new();
        let known_order = ["orchestrators", "workers", "tools"];

        for name in &known_order {
            if self.config.groups.contains_key(*name) {
                result.push(name.to_string());
            }
        }

        // Custom groups (not in known_order), sorted alphabetically
        let mut custom: Vec<&String> = self
            .config
            .groups
            .keys()
            .filter(|k| !known_order.contains(&k.as_str()))
            .collect();
        custom.sort();
        for name in custom {
            result.push(name.clone());
        }

        // Check for ungrouped golems
        let all_grouped: std::collections::HashSet<&str> = self
            .config
            .groups
            .values()
            .flat_map(|names| names.iter().map(|s| s.as_str()))
            .collect();
        let has_ungrouped = self
            .config
            .golem
            .iter()
            .any(|g| !all_grouped.contains(g.name.as_str()));
        if has_ungrouped {
            result.push("OTHER".into());
        }

        result
    }

    /// Returns golem configs belonging to a group. For "OTHER", returns ungrouped golems.
    pub fn golems_in_group(&self, group: &str) -> Vec<&crate::config::GolemConfig> {
        if group == "OTHER" {
            let all_grouped: std::collections::HashSet<&str> = self
                .config
                .groups
                .values()
                .flat_map(|names| names.iter().map(|s| s.as_str()))
                .collect();
            return self
                .config
                .golem
                .iter()
                .filter(|g| !all_grouped.contains(g.name.as_str()))
                .collect();
        }
        let names = match self.config.groups.get(group) {
            Some(names) => names,
            None => return vec![],
        };
        names
            .iter()
            .filter_map(|name| self.config.golem.iter().find(|g| &g.name == name))
            .collect()
    }

    /// Find the best matching external agent state for a slot label.
    /// Matches by checking if any state file's surface name contains the label (case-insensitive).
    fn agent_state_for_slot(&self, label: &str) -> Option<&agent_state::AgentExternalState> {
        let label_lower = label.to_lowercase().replace(' ', "-");
        // Try exact match first
        if let Some(state) = self.agent_states.get(&label_lower) {
            return Some(state);
        }
        // Try partial match (surface name contains label)
        self.agent_states
            .iter()
            .find(|(surface, _)| surface.contains(&label_lower) || label_lower.contains(surface.as_str()))
            .map(|(_, state)| state)
    }

    fn terminal_content_text(terminal: &iced_term::Terminal) -> String {
        let content = terminal.renderable_content();
        let mut text = String::new();
        let mut line = String::new();
        let mut last_non_space = 0usize;
        let mut current_line = None;

        for indexed in content.grid.display_iter() {
            let line_no = indexed.point.line.0;
            if current_line != Some(line_no) {
                if current_line.is_some() {
                    line.truncate(last_non_space);
                    text.push_str(&line);
                    text.push('\n');
                    line.clear();
                    last_non_space = 0;
                }
                current_line = Some(line_no);
            }

            line.push(indexed.cell.c);
            for ch in indexed.cell.zerowidth().into_iter().flatten() {
                line.push(*ch);
            }

            if !matches!(indexed.cell.c, ' ') {
                last_non_space = line.len();
            }
        }

        if current_line.is_some() {
            line.truncate(last_non_space);
            text.push_str(&line);
        }

        text.trim_end_matches('\n').to_string()
    }

    fn sync_test_state(&self) {
        let mut ts = self.test_state.lock().unwrap();
        ts.active_tab = self.active_slot_idx().unwrap_or(0);
        ts.split_active = self.panes.len() > 1;

        // For backward compat, find the "secondary" pane's slot
        if self.panes.len() > 1 {
            if let Some(focus) = self.focus {
                // secondary = first pane that isn't the focused one
                ts.split_secondary = self
                    .panes
                    .iter()
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
            ts.slots[i].content = slot
                .terminal
                .as_ref()
                .map(Self::terminal_content_text)
                .unwrap_or_default();
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
                        self.sync_test_state();
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
                let slot = AgentSlot::new(id, self.base_cmd.clone(), label, &self.config);
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
                let pane_updates: Vec<(pane_grid::Pane, usize)> = self
                    .panes
                    .iter()
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
                        let adjacent = self
                            .panes
                            .adjacent(focused, primary)
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
                    self.sync_test_state();
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
                    let slot = AgentSlot::new(id, self.base_cmd.clone(), label, &self.config);
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
                        let to_close = self
                            .panes
                            .iter()
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
                        (0..self.slots.len()).find(|&i| i != current).unwrap_or(0)
                    } else {
                        // Create a new slot
                        let id = self.next_slot_id;
                        self.next_slot_id += 1;
                        let label = format!("Agent {}", id + 1);
                        let slot = AgentSlot::new(id, self.base_cmd.clone(), label, &self.config);
                        self.slots.push(slot);
                        let mut ts = self.test_state.lock().unwrap();
                        ts.slots.push(crate::test_harness::SlotState::default());
                        drop(ts);
                        self.slots.len() - 1
                    };
                    if let Some((new_pane, _)) =
                        self.panes
                            .split(pane_grid::Axis::Vertical, focused, secondary)
                    {
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

            Message::ToggleGroup(name) => {
                let collapsed = self.collapsed_groups.entry(name).or_insert(false);
                *collapsed = !*collapsed;
                Task::none()
            }

            Message::LaunchGolem(name) => {
                let golem = self.config.golem.iter().find(|g| g.name == name).cloned();
                if let Some(golem) = golem {
                    let id = self.next_slot_id;
                    self.next_slot_id += 1;
                    let mut slot =
                        AgentSlot::new(id, golem.command.clone(), golem.name.clone(), &self.config);

                    // Set working directory to golem's repo
                    let repo_path = crate::config::expand_path(&golem.repo);
                    slot.term_settings.backend.working_directory = Some(repo_path);

                    self.slots.push(slot);
                    let new_idx = self.slots.len() - 1;

                    // Update focused pane to show the new slot
                    if let Some(focused) = self.focus {
                        if let Some(slot_ref) = self.panes.get_mut(focused) {
                            *slot_ref = new_idx;
                        }
                    }

                    let mut ts = self.test_state.lock().unwrap();
                    ts.slots.push(crate::test_harness::SlotState::default());
                    drop(ts);
                }
                Task::none()
            }

            Message::AgentStateTick => {
                self.agent_states = agent_state::read_all_states(&self.agent_state_dir);
                Task::none()
            }

            Message::ConfigReloaded(new_config) => {
                eprintln!(
                    "[config] reloaded: {} golems, {} groups",
                    new_config.golem.len(),
                    new_config.groups.len()
                );
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
            keyboard::Event::KeyPressed { key, modifiers, .. }
                if modifiers.command() && modifiers.alt() =>
            {
                match key {
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Message::SwitchTab(-1),
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Message::SwitchTab(1),
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Message::SwitchTab(-1),
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Message::SwitchTab(1),
                    _ => Message::KeyboardIgnored,
                }
            }
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
            } if c.as_ref() == "d" && modifiers.command() => Message::ToggleSplit,
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

        // Agent state polling (every 2 seconds)
        subscriptions.push(
            iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::AgentStateTick),
        );

        // Config file watcher for hot-reload
        subscriptions.push(crate::config::watch_config().map(Message::ConfigReloaded));

        Subscription::batch(subscriptions)
    }

    // ── View ─────────────────────────────────────────────────────────────────

    pub fn view(&self) -> iced::Element<'_, Message> {
        let focus = self.focus;
        let focus_border_color = parse_hex_color(&self.config.ui.colors.focus_border);
        let pane_spacing = self.config.ui.pane_spacing;

        let pane_grid_widget = PaneGrid::new(&self.panes, |pane_id, &slot_idx, _is_maximized| {
            let is_focused = focus == Some(pane_id);
            let content = self.view_pane_content(slot_idx, is_focused);

            pane_grid::Content::new(content).style(move |_theme| {
                let border_color = if is_focused {
                    focus_border_color
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
        .spacing(pane_spacing)
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

    fn view_pane_content(&self, slot_idx: usize, _is_focused: bool) -> iced::Element<'_, Message> {
        let text_secondary = parse_hex_color(&self.config.ui.colors.text_secondary);
        let bg_primary = parse_hex_color(&self.config.ui.colors.bg_primary);
        let font_small = self.config.ui.font.small;
        let font_tiny = self.config.ui.font.tiny;

        if slot_idx >= self.slots.len() {
            return container(text("Invalid pane").color(text_secondary))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into();
        }

        let slot = &self.slots[slot_idx];

        let terminal_view: iced::Element<'_, Message> = if let Some(ref term) = slot.terminal {
            container(TerminalView::show(term).map(Message::TermEvent))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(bg_primary)),
                    ..Default::default()
                })
                .into()
        } else {
            // Idle state — show launch prompt
            let launch_text = column![
                text("Terminal idle").size(font_small).color(text_secondary),
                text("Press Enter or click to launch")
                    .size(font_tiny)
                    .color(text_secondary),
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
            .style(move |_theme, _status| iced::widget::button::Style {
                background: Some(iced::Background::Color(bg_primary)),
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
        let colors = &self.config.ui.colors;
        let font = &self.config.ui.font;
        let text_secondary = parse_hex_color(&colors.text_secondary);
        let text_tab_active = parse_hex_color(&colors.text_tab_active);
        let status_running = parse_hex_color(&colors.status_running);
        let status_pending = parse_hex_color(&colors.status_pending);
        let status_idle = parse_hex_color(&colors.status_idle);
        let status_error = parse_hex_color(&colors.status_error);
        let bg_tab_active = parse_hex_color(&colors.bg_tab_active);
        let bg_tab_hover = parse_hex_color(&colors.bg_tab_hover);
        let accent_color = parse_hex_color(&colors.accent);
        let focus_border = parse_hex_color(&colors.focus_border);
        let bg_sidebar = parse_hex_color_alpha(&colors.bg_sidebar, 0.7);
        let sidebar_width = self.config.ui.sidebar_width;
        let font_group = font.group;
        let font_tab = font.tab;
        let font_small = font.small;
        let font_tiny = font.tiny;

        let mut sidebar_content = column![].spacing(2.0).padding([SPACING_NORMAL, 0.0]);

        let active_slot = self.active_slot_idx();
        let visible_slots: Vec<usize> = self.panes.iter().map(|(_, idx)| *idx).collect();

        let groups = self.ordered_groups();
        let has_golem_presets = !self.config.golem.is_empty();

        if has_golem_presets {
            // Render grouped golem presets
            for group_name in &groups {
                let collapsed = self
                    .collapsed_groups
                    .get(group_name)
                    .copied()
                    .unwrap_or(false);
                let arrow = if collapsed { ">" } else { "v" };
                let header_label = format!("{} {}", arrow, group_name.to_uppercase());

                let group_name_clone = group_name.clone();
                let group_header = button(
                    container(text(header_label).size(font_group).color(text_secondary))
                        .padding([SPACING_TIGHT, SPACING_NORMAL])
                        .width(Length::Fill),
                )
                .style(move |_theme, status| {
                    let mut style = iced::widget::button::Style {
                        background: None,
                        ..Default::default()
                    };
                    if matches!(status, iced::widget::button::Status::Hovered) {
                        style.background = Some(iced::Background::Color(bg_tab_hover));
                    }
                    style
                })
                .padding(0)
                .width(Length::Fill)
                .on_press(Message::ToggleGroup(group_name_clone));

                sidebar_content = sidebar_content.push(group_header);

                if collapsed {
                    continue;
                }

                let golems = self.golems_in_group(group_name);
                for golem in golems {
                    let golem_color = parse_hex_color(&golem.color);
                    let golem_name = golem.name.clone();

                    // Check if this golem has a running slot
                    let running_slot = self
                        .slots
                        .iter()
                        .enumerate()
                        .find(|(_, s)| s.label == golem.name);

                    let (dot_color, label_color, bg, border_color, on_press, slot_idx) =
                        if let Some((idx, slot)) = running_slot {
                            let is_active = active_slot == Some(idx);
                            let is_in_split = !is_active && visible_slots.contains(&idx);
                            let dc = match slot.status {
                                SlotStatus::Running => status_running,
                                SlotStatus::Pending => status_pending,
                                SlotStatus::Idle => status_idle,
                            };
                            let lc = if is_active || is_in_split {
                                text_tab_active
                            } else {
                                text_secondary
                            };
                            let bg = if is_active {
                                bg_tab_active
                            } else if is_in_split {
                                bg_tab_hover
                            } else {
                                iced::Color::TRANSPARENT
                            };
                            let bc = if is_active {
                                golem_color
                            } else if is_in_split {
                                focus_border
                            } else {
                                iced::Color::TRANSPARENT
                            };
                            (dc, lc, bg, bc, Message::SelectTab(idx), Some(idx))
                        } else {
                            (
                                status_idle,
                                text_secondary,
                                iced::Color::TRANSPARENT,
                                iced::Color::TRANSPARENT,
                                Message::LaunchGolem(golem_name.clone()),
                                None,
                            )
                        };

                    // Override dot color from external agent state if available
                    let ext_state = self.agent_state_for_slot(&golem.name);
                    let dot_color = if let Some(ext) = ext_state {
                        match ext.status_color_hint() {
                            "running" => status_running,
                            "pending" => status_pending,
                            "error" => status_error,
                            _ => dot_color,
                        }
                    } else {
                        dot_color
                    };

                    let icon_text = text(&golem.icon).size(font_tab);
                    let label_widget = text(&golem.name).size(font_tab).color(label_color);
                    let dot = text("●").size(STATUS_DOT_SIZE).color(dot_color);

                    let top_row =
                        row![icon_text, label_widget, Space::new().width(Length::Fill), dot]
                            .spacing(SPACING_TIGHT)
                            .align_y(iced::Alignment::Center);

                    // Build tab content: top row + optional subtitle from agent state
                    let tab_content: iced::Element<'_, Message> =
                        if let Some(ext) = ext_state {
                            let summary = ext.sidebar_summary();
                            if summary.is_empty() {
                                top_row.into()
                            } else {
                                let subtitle = text(summary)
                                    .size(font_tiny)
                                    .color(text_secondary);
                                column![top_row, subtitle].spacing(1).into()
                            }
                        } else {
                            top_row.into()
                        };

                    let is_bordered = slot_idx.map_or(false, |idx| {
                        active_slot == Some(idx) || visible_slots.contains(&idx)
                    });

                    let tab_container = container(tab_content)
                        .style(move |_theme| container::Style {
                            background: Some(iced::Background::Color(bg)),
                            border: iced::Border {
                                color: border_color,
                                width: if is_bordered { 2.0 } else { 0.0 },
                                radius: iced::border::radius(0.0)
                                    .top_right(BORDER_RADIUS)
                                    .bottom_right(BORDER_RADIUS),
                            },
                            ..Default::default()
                        })
                        .padding([SPACING_TIGHT, SPACING_NORMAL + 8.0]) // indent under group
                        .width(Length::Fill);

                    let tab_element: iced::Element<'_, Message> = if let Some(idx) = slot_idx {
                        mouse_area(
                            button(tab_container)
                                .style(|_theme, _status| iced::widget::button::Style {
                                    background: None,
                                    ..Default::default()
                                })
                                .padding(0)
                                .on_press(on_press),
                        )
                        .on_middle_press(Message::CloseTab(idx))
                        .into()
                    } else {
                        button(tab_container)
                            .style(|_theme, _status| iced::widget::button::Style {
                                background: None,
                                ..Default::default()
                            })
                            .padding(0)
                            .on_press(on_press)
                            .into()
                    };

                    sidebar_content = sidebar_content.push(tab_element);
                }
            }

            // Separator before ad-hoc tabs
            if !self.ad_hoc_slots().is_empty() {
                sidebar_content = sidebar_content.push(rule::horizontal(RULE_THICKNESS));
                let adhoc_header = container(text("AD-HOC").size(font_group).color(text_secondary))
                    .padding([SPACING_TIGHT, SPACING_NORMAL]);
                sidebar_content = sidebar_content.push(adhoc_header);
            }
        }

        // Ad-hoc tabs (slots not tied to a golem preset) or all tabs if no presets
        let adhoc_slots: Vec<(usize, &AgentSlot)> = if has_golem_presets {
            self.ad_hoc_slots()
        } else {
            self.slots.iter().enumerate().collect()
        };

        if !has_golem_presets {
            // Legacy: flat "AGENTS" header when no golem presets configured
            let group_header = container(text("AGENTS").size(font_group).color(text_secondary))
                .padding([SPACING_TIGHT, SPACING_NORMAL]);
            sidebar_content = sidebar_content.push(group_header);
        }

        for (i, slot) in adhoc_slots {
            let is_active = active_slot == Some(i);
            let is_in_split = !is_active && visible_slots.contains(&i);

            let dot_color = match slot.status {
                SlotStatus::Running => status_running,
                SlotStatus::Pending => status_pending,
                SlotStatus::Idle => status_idle,
            };
            let dot = text("●").size(STATUS_DOT_SIZE).color(dot_color);

            let label_color = if is_active || is_in_split {
                text_tab_active
            } else {
                text_secondary
            };
            let label = text(&slot.label).size(font_tab).color(label_color);

            let tab_row = row![dot, label]
                .spacing(SPACING_NORMAL)
                .align_y(iced::Alignment::Center);

            let bg = if is_active {
                bg_tab_active
            } else if is_in_split {
                bg_tab_hover
            } else {
                iced::Color::TRANSPARENT
            };

            let border_color = if is_active {
                accent_color
            } else if is_in_split {
                focus_border
            } else {
                iced::Color::TRANSPARENT
            };

            let tab_container = container(tab_row)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(bg)),
                    border: iced::Border {
                        color: border_color,
                        width: if is_active || is_in_split { 2.0 } else { 0.0 },
                        radius: iced::border::radius(0.0)
                            .top_right(BORDER_RADIUS)
                            .bottom_right(BORDER_RADIUS),
                    },
                    ..Default::default()
                })
                .padding([SPACING_TIGHT, SPACING_NORMAL])
                .width(Length::Fill);

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
        sidebar_content =
            sidebar_content.push(Space::new().width(Length::Fill).height(Length::Fill));

        // New tab button at bottom
        let new_tab_btn = button(
            row![
                text("+").size(font_tab).color(text_secondary),
                text("New Agent").size(font_small).color(text_secondary),
            ]
            .spacing(SPACING_NORMAL)
            .align_y(iced::Alignment::Center),
        )
        .style(move |_theme, status| {
            let mut style = iced::widget::button::Style {
                background: None,
                ..Default::default()
            };
            if matches!(status, iced::widget::button::Status::Hovered) {
                style.background = Some(iced::Background::Color(bg_tab_hover));
            }
            style
        })
        .padding([SPACING_TIGHT, SPACING_NORMAL])
        .width(Length::Fill)
        .on_press(Message::NewTab);

        sidebar_content = sidebar_content.push(new_tab_btn);

        container(sidebar_content)
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(bg_sidebar)),
                ..Default::default()
            })
            .width(sidebar_width)
            .height(Length::Fill)
            .into()
    }

    // ── Bottom Bar ───────────────────────────────────────────────────────────

    fn view_bottom_bar<'a>(&self, slot: &'a AgentSlot) -> iced::Element<'a, Message> {
        let colors = &self.config.ui.colors;
        let status_running = parse_hex_color(&colors.status_running);
        let status_pending = parse_hex_color(&colors.status_pending);
        let status_idle = parse_hex_color(&colors.status_idle);
        let text_secondary = parse_hex_color(&colors.text_secondary);
        let bg_secondary = parse_hex_color(&colors.bg_secondary);
        let font_tiny = self.config.ui.font.tiny;
        let bottom_bar_height = self.config.ui.bottom_bar_height;

        let status = slot.status_text();

        let status_dot_color = match slot.status {
            SlotStatus::Running => status_running,
            SlotStatus::Pending => status_pending,
            SlotStatus::Idle => status_idle,
        };
        let status_dot = text("●").size(STATUS_DOT_SIZE).color(status_dot_color);
        let status_label = text(status).size(font_tiny).color(text_secondary);

        let bar = row![status_dot, status_label, Space::new().width(Length::Fill),]
            .spacing(SPACING_TIGHT)
            .align_y(iced::Alignment::Center);

        container(bar.width(Length::Fill).padding([2.0, SPACING_NORMAL]))
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(bg_secondary)),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(bottom_bar_height)
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
    let mtm =
        MainThreadMarker::new().expect("apply_macos_vibrancy must be called from main thread");

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
        ) -> Result<
            iced::window::raw_window_handle::WindowHandle<'_>,
            iced::window::raw_window_handle::HandleError,
        > {
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
    // Remove CLAUDECODE env var so child terminals can launch Claude Code
    // (otherwise they think they're nested inside a Claude Code session)
    std::env::remove_var("CLAUDECODE");

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
        assert_eq!(
            active_slot(&state),
            1,
            "SwitchTab(-1) from 0 should wrap to last"
        );
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
        let other = state
            .panes
            .iter()
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

    // ── Phase 4: RepoGolem Sidebar Tests ────────────────────────────────────

    fn state_with_golems() -> State {
        let mut state = State::new(vec!["echo".into()]);
        state.config.golem = vec![
            crate::config::GolemConfig {
                name: "orcClaude".into(),
                repo: "~/Gits/orchestrator".into(),
                icon: "O".into(),
                color: "#7C3AED".into(),
                golem_type: crate::config::GolemType::Orchestrator,
                command: vec!["echo".into(), "orc".into()],
                context_file: None,
            },
            crate::config::GolemConfig {
                name: "brainClaude".into(),
                repo: "~/Gits/brainlayer".into(),
                icon: "B".into(),
                color: "#06B6D4".into(),
                golem_type: crate::config::GolemType::Worker,
                command: vec!["echo".into(), "brain".into()],
                context_file: None,
            },
            crate::config::GolemConfig {
                name: "Cursor Audit".into(),
                repo: "~/Gits".into(),
                icon: "C".into(),
                color: "#6B7280".into(),
                golem_type: crate::config::GolemType::Tool,
                command: vec!["echo".into(), "cursor".into()],
                context_file: None,
            },
        ];
        state.config.groups = std::collections::HashMap::from([
            ("orchestrators".into(), vec!["orcClaude".into()]),
            ("workers".into(), vec!["brainClaude".into()]),
            ("tools".into(), vec!["Cursor Audit".into()]),
        ]);
        state
    }

    #[test]
    fn collapsed_groups_initialized_empty() {
        let state = state_with_golems();
        assert!(
            state.collapsed_groups.is_empty(),
            "all groups start expanded"
        );
    }

    #[test]
    fn toggle_group_collapses_and_expands() {
        let mut state = state_with_golems();
        let _ = state.update(Message::ToggleGroup("workers".into()));
        assert_eq!(state.collapsed_groups.get("workers"), Some(&true));
        let _ = state.update(Message::ToggleGroup("workers".into()));
        assert_eq!(state.collapsed_groups.get("workers"), Some(&false));
    }

    #[test]
    fn launch_golem_creates_slot_with_golem_settings() {
        let mut state = state_with_golems();
        let initial_slots = state.slots.len();
        let _ = state.update(Message::LaunchGolem("orcClaude".into()));
        assert_eq!(state.slots.len(), initial_slots + 1);
        let new_slot = state.slots.last().unwrap();
        assert_eq!(new_slot.label, "orcClaude");
    }

    #[test]
    fn launch_golem_unknown_name_is_noop() {
        let mut state = state_with_golems();
        let initial_slots = state.slots.len();
        let _ = state.update(Message::LaunchGolem("nonexistent".into()));
        assert_eq!(
            state.slots.len(),
            initial_slots,
            "unknown golem should not create slot"
        );
    }

    #[test]
    fn launch_golem_sets_working_directory() {
        let mut state = state_with_golems();
        let _ = state.update(Message::LaunchGolem("orcClaude".into()));
        let new_slot = state.slots.last().unwrap();
        // The BackendSettings should have the expanded repo path as working_directory
        let wd = &new_slot.term_settings.backend.working_directory;
        assert!(wd.is_some(), "working directory should be set");
        let wd_path = wd.as_ref().unwrap();
        assert!(
            wd_path.to_str().unwrap().contains("orchestrator"),
            "working directory should contain repo path, got: {:?}",
            wd_path
        );
    }

    #[test]
    fn ordered_groups_returns_correct_order() {
        let state = state_with_golems();
        let groups = state.ordered_groups();
        // orchestrators first, workers second, tools third
        assert_eq!(groups[0], "orchestrators");
        assert_eq!(groups[1], "workers");
        assert_eq!(groups[2], "tools");
    }

    #[test]
    fn ungrouped_golems_go_to_other() {
        let mut state = state_with_golems();
        // Add a golem not in any group
        state.config.golem.push(crate::config::GolemConfig {
            name: "stray".into(),
            repo: "~/stray".into(),
            icon: "S".into(),
            color: "#000000".into(),
            golem_type: crate::config::GolemType::Worker,
            command: vec![],
            context_file: None,
        });
        let groups = state.ordered_groups();
        assert!(
            groups.contains(&"OTHER".to_string()),
            "ungrouped golems should create OTHER group"
        );
    }

    #[test]
    fn golems_in_group_returns_matching_configs() {
        let state = state_with_golems();
        let workers = state.golems_in_group("workers");
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].name, "brainClaude");
    }
}
