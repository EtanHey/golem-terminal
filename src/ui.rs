// Golem Terminal V2 — Tab-based terminal multiplexer built on Iced + iced_term
//
// AIDEV-NOTE: V2 uses iced_term (alacritty_terminal backend) for proper terminal
// rendering with colors, cursor, selection, scrollback. Sidebar navigation
// (Zen browser style) replaces top tab bar.

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

// Colors (iTerm-inspired dark theme)
const BG_PRIMARY: iced::Color = iced::Color::from_rgb(0.11, 0.11, 0.14);
const BG_SECONDARY: iced::Color = iced::Color::from_rgb(0.15, 0.15, 0.19);
const BG_SIDEBAR: iced::Color = iced::Color::from_rgb(0.12, 0.12, 0.15);
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
    // Tab management
    SelectTab(usize),
    NewTab,
    CloseTab(usize),
    SwitchTab(i32),

    // Split screen
    ToggleSplit,
    FocusPane(PaneSide),

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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaneSide {
    Primary,
    Secondary,
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
                // AIDEV-NOTE: Register bindings that swallow our Cmd shortcuts so
                // iced_term doesn't forward the letter to the PTY. BindingAction::LinkOpen
                // falls to the no-op `_ => {}` branch in iced_term's keyboard handler.
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
// AIDEV-NOTE: These bindings prevent Cmd+letter shortcuts from being forwarded
// to the PTY by iced_term. Without them, pressing Cmd+D would both toggle split
// AND type 'd' into the terminal. BindingAction::LinkOpen is a no-op in
// iced_term's keyboard handler (falls to `_ => {}`).

fn gui_shortcut_bindings() -> Vec<(Binding<InputKind>, BindingAction)> {
    use iced::keyboard::Modifiers;

    let cmd = Modifiers::COMMAND;

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
        swallow("d", cmd),   // Cmd+D → toggle split
        swallow("b", cmd),   // Cmd+B → toggle sidebar
        swallow("t", cmd),   // Cmd+T → new tab
        swallow("q", cmd),   // Cmd+Q → quit
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
    pub active_tab: usize,
    pub split_active: bool,
    pub split_secondary: usize,
    pub focused_pane: PaneSide,
    pub sidebar_visible: bool,
    pub base_cmd: Vec<String>,
    pub next_slot_id: usize,
    pub test_state: Arc<Mutex<crate::test_harness::TestState>>,
}

impl State {
    pub fn new(cmd: Vec<String>) -> Self {
        let slot = AgentSlot::new(0, cmd.clone(), "Agent 1".into());
        let test_state = Arc::new(Mutex::new(crate::test_harness::TestState {
            slots: vec![crate::test_harness::SlotState::default()],
            active_tab: 0,
            split_active: false,
            split_secondary: 0,
            death_cries_enabled: false,
        }));

        Self {
            slots: vec![slot],
            active_tab: 0,
            split_active: false,
            split_secondary: 0,
            focused_pane: PaneSide::Primary,
            sidebar_visible: true,
            base_cmd: cmd,
            next_slot_id: 1,
            test_state,
        }
    }

    fn active_slot(&self) -> Option<usize> {
        if self.slots.is_empty() {
            None
        } else {
            Some(self.active_tab.min(self.slots.len() - 1))
        }
    }

    fn sync_test_state(&self) {
        let mut ts = self.test_state.lock().unwrap();
        ts.active_tab = self.active_tab;
        ts.split_active = self.split_active;
        ts.split_secondary = self.split_secondary;

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
        let task = match message {
            Message::LaunchSlot(idx) => {
                if idx < self.slots.len() && self.slots[idx].status == SlotStatus::Idle {
                    self.slots[idx].launch();
                    // Focus the newly launched terminal
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

            Message::SendInput(_idx, _data) => {
                // AIDEV-TODO: iced_term's backend module is private, so we can't
                // construct Write commands directly. Input goes through iced_term's
                // keyboard handling natively. Test harness send_input needs iced_term
                // fork or upstream PR to expose backend::Command::Write.
                Task::none()
            }

            Message::SelectTab(idx) => {
                if idx < self.slots.len() {
                    if self.split_active {
                        if idx == self.split_secondary {
                            // Clicked the secondary tab → swap primary and secondary
                            self.split_secondary = self.active_tab;
                            self.active_tab = idx;
                        } else if idx == self.active_tab {
                            // Clicked the already-active primary → no change
                        } else {
                            // Clicked a third tab → it becomes primary, secondary stays
                            self.active_tab = idx;
                        }
                    } else {
                        self.active_tab = idx;
                    }
                    self.focused_pane = PaneSide::Primary;
                    // Focus the primary terminal widget
                    if let Some(ref term) = self.slots[self.active_tab].terminal {
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
                self.active_tab = self.slots.len() - 1;

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
                if self.active_tab >= self.slots.len() {
                    self.active_tab = self.slots.len() - 1;
                }
                if self.split_secondary >= self.slots.len() {
                    self.split_secondary = self.slots.len().saturating_sub(1);
                }
                Task::none()
            }

            Message::SwitchTab(delta) => {
                // In split mode, Cmd+Alt+Arrow switches focus between panes
                if self.split_active {
                    let new_side = match self.focused_pane {
                        PaneSide::Primary => PaneSide::Secondary,
                        PaneSide::Secondary => PaneSide::Primary,
                    };
                    self.focused_pane = new_side;
                    let idx = match new_side {
                        PaneSide::Primary => self.active_tab,
                        PaneSide::Secondary => self.split_secondary,
                    };
                    if idx < self.slots.len() {
                        if let Some(ref term) = self.slots[idx].terminal {
                            return TerminalView::focus(term.widget_id().clone());
                        }
                    }
                    return Task::none();
                }
                // Not in split mode — cycle tabs normally
                let len = self.slots.len() as i32;
                if len == 0 {
                    return Task::none();
                }
                let current = self.active_tab as i32;
                let new_idx = (current + delta).rem_euclid(len) as usize;
                self.active_tab = new_idx;
                self.focused_pane = PaneSide::Primary;
                if let Some(ref term) = self.slots[new_idx].terminal {
                    return TerminalView::focus(term.widget_id().clone());
                }
                Task::none()
            }

            Message::ToggleSplit => {
                self.split_active = !self.split_active;
                if self.split_active && self.slots.len() > 1 {
                    self.split_secondary = if self.active_tab == 0 { 1 } else { 0 };
                    // Focus the NEW secondary pane (the one just added to split)
                    self.focused_pane = PaneSide::Secondary;
                    if let Some(ref term) = self.slots[self.split_secondary].terminal {
                        return TerminalView::focus(term.widget_id().clone());
                    }
                } else {
                    // Split closed — focus primary
                    self.focused_pane = PaneSide::Primary;
                    if let Some(ref term) = self.slots[self.active_tab].terminal {
                        return TerminalView::focus(term.widget_id().clone());
                    }
                }
                Task::none()
            }

            Message::FocusPane(side) => {
                self.focused_pane = side;
                let idx = match side {
                    PaneSide::Primary => self.active_tab,
                    PaneSide::Secondary => self.split_secondary,
                };
                if idx < self.slots.len() {
                    if let Some(ref term) = self.slots[idx].terminal {
                        return TerminalView::focus(term.widget_id().clone());
                    }
                }
                Task::none()
            }

            Message::TermEvent(iced_term::Event::BackendCall(id, cmd)) => {
                if let Some(slot) = self.slots.iter_mut().find(|s| s.id as u64 == id) {
                    if let Some(ref mut term) = slot.terminal {
                        let action = term.handle(iced_term::Command::ProxyToBackend(cmd));
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
                // Re-focus current terminal to prevent dual-focus
                let focus_idx = match self.focused_pane {
                    PaneSide::Primary => self.active_tab,
                    PaneSide::Secondary => self.split_secondary,
                };
                if focus_idx < self.slots.len() {
                    if let Some(ref term) = self.slots[focus_idx].terminal {
                        return TerminalView::focus(term.widget_id().clone());
                    }
                }
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
            // Cmd+Alt+Left/Right → SwitchTab (update handler checks split state)
            keyboard::Event::KeyPressed {
                key, modifiers, ..
            } if modifiers.command() && modifiers.alt() => match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                    Message::SwitchTab(-1)
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                    Message::SwitchTab(1)
                }
                // Cmd+Alt+Up/Down → also cycle tabs
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                    Message::SwitchTab(-1)
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
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
            // Cmd+D → toggle split
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(ref c),
                modifiers,
                ..
            } if c.as_ref() == "d" && modifiers.command() => Message::ToggleSplit,
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

        Subscription::batch(subscriptions)
    }

    // ── View ─────────────────────────────────────────────────────────────────

    pub fn view(&self) -> iced::Element<'_, Message> {
        let content_area = if self.split_active && self.slots.len() > 1 {
            let left_idx = self.active_tab;
            let right_idx = self.split_secondary;

            let left_panel = self.view_terminal_panel(left_idx, PaneSide::Primary);
            let right_panel = self.view_terminal_panel(right_idx, PaneSide::Secondary);

            row![left_panel, rule::vertical(RULE_THICKNESS), right_panel]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(idx) = self.active_slot() {
            self.view_terminal_panel(idx, PaneSide::Primary)
        } else {
            container(text("No tabs open").color(TEXT_SECONDARY))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        };

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

        // Tab entries
        for (i, slot) in self.slots.iter().enumerate() {
            let is_active = i == self.active_tab;
            let is_split_secondary = self.split_active && i == self.split_secondary;

            // Status dot
            let dot_color = match slot.status {
                SlotStatus::Running => STATUS_RUNNING,
                SlotStatus::Pending => STATUS_PENDING,
                SlotStatus::Idle => STATUS_IDLE,
            };
            let dot = text("●").size(STATUS_DOT_SIZE).color(dot_color);

            // Label
            let label_color = if is_active || is_split_secondary {
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
            } else if is_split_secondary {
                BG_TAB_HOVER
            } else {
                iced::Color::TRANSPARENT
            };

            // Left accent border for active tab
            let border_color = if is_active {
                ACCENT_COLOR
            } else if is_split_secondary {
                FOCUS_BORDER_COLOR
            } else {
                iced::Color::TRANSPARENT
            };

            let tab_container = container(tab_row)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(bg)),
                    border: iced::Border {
                        color: border_color,
                        width: if is_active || is_split_secondary {
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

    // ── Terminal Panel ───────────────────────────────────────────────────────

    fn view_terminal_panel(
        &self,
        idx: usize,
        pane_side: PaneSide,
    ) -> iced::Element<'_, Message> {
        let slot = &self.slots[idx];
        let is_focused = self.focused_pane == pane_side;
        let width = if self.split_active {
            Length::FillPortion(1)
        } else {
            Length::Fill
        };

        let terminal_view: iced::Element<'_, Message> = if let Some(ref term) = slot.terminal {
            container(
                TerminalView::show(term).map(Message::TermEvent),
            )
            .width(Length::Fill)
            .height(Length::Fill)
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
            .on_press(Message::LaunchSlot(idx));

            launch_area.into()
        };

        let bottom_bar = self.view_bottom_bar(slot);

        // Focus border indicator
        let border_color = if is_focused && self.split_active {
            FOCUS_BORDER_COLOR
        } else {
            iced::Color::TRANSPARENT
        };

        let panel = column![terminal_view, rule::horizontal(RULE_THICKNESS), bottom_bar]
            .width(width)
            .height(Length::Fill);

        let styled_panel = container(panel)
            .style(move |_theme| container::Style {
                border: iced::Border {
                    color: border_color,
                    width: if is_focused && self.split_active {
                        2.0
                    } else {
                        0.0
                    },
                    ..Default::default()
                },
                ..Default::default()
            })
            .width(width)
            .height(Length::Fill);

        if self.split_active {
            mouse_area(styled_panel)
                .on_press(Message::FocusPane(pane_side))
                .into()
        } else {
            styled_panel.into()
        }
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
    fn switch_tab_in_split_toggles_focus() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::SelectTab(0));
        let _ = state.update(Message::ToggleSplit);
        assert!(state.split_active);
        // ToggleSplit focuses secondary
        assert_eq!(state.focused_pane, PaneSide::Secondary);
        // SwitchTab in split mode should toggle focus, not change tabs
        let _ = state.update(Message::SwitchTab(1));
        assert_eq!(state.focused_pane, PaneSide::Primary);
        assert_eq!(state.active_tab, 0, "tabs should not change in split SwitchTab");
        let _ = state.update(Message::SwitchTab(-1));
        assert_eq!(state.focused_pane, PaneSide::Secondary);
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
        assert_eq!(state.active_tab, 0);
        let _ = state.update(Message::SwitchTab(-1));
        assert_eq!(state.active_tab, 1, "SwitchTab(-1) from 0 should wrap to last");
    }

    #[test]
    fn close_tab_clamps_active_tab() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        assert_eq!(state.active_tab, 1);
        let _ = state.update(Message::CloseTab(1));
        assert_eq!(state.active_tab, 0, "active_tab must clamp after closing last tab");
    }

    #[test]
    fn close_tab_clamps_split_secondary() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        state.split_secondary = 1;
        let _ = state.update(Message::CloseTab(1));
        assert!(
            state.split_secondary < state.slots.len(),
            "split_secondary must clamp after tab removal"
        );
    }

    #[test]
    fn select_tab_changes_active() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::NewTab);
        assert_eq!(state.active_tab, 2);
        let _ = state.update(Message::SelectTab(0));
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn focus_pane_switches() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::ToggleSplit);
        // ToggleSplit focuses the new secondary pane
        assert_eq!(state.focused_pane, PaneSide::Secondary);
        let _ = state.update(Message::FocusPane(PaneSide::Primary));
        assert_eq!(state.focused_pane, PaneSide::Primary);
        let _ = state.update(Message::FocusPane(PaneSide::Secondary));
        assert_eq!(state.focused_pane, PaneSide::Secondary);
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

    #[test]
    fn split_select_secondary_swaps() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::NewTab);
        // Tab 0 is primary, enable split
        let _ = state.update(Message::SelectTab(0));
        let _ = state.update(Message::ToggleSplit);
        assert_eq!(state.active_tab, 0);
        assert_eq!(state.split_secondary, 1);
        // Click the secondary tab (1) → should swap
        let _ = state.update(Message::SelectTab(1));
        assert_eq!(state.active_tab, 1, "secondary should become primary");
        assert_eq!(state.split_secondary, 0, "old primary should become secondary");
    }

    #[test]
    fn split_select_third_tab_keeps_secondary() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::SelectTab(0));
        let _ = state.update(Message::ToggleSplit);
        assert_eq!(state.split_secondary, 1);
        // Click tab 2 (not in split) → becomes primary, secondary stays
        let _ = state.update(Message::SelectTab(2));
        assert_eq!(state.active_tab, 2);
        assert_eq!(state.split_secondary, 1, "secondary should not change");
    }
}
