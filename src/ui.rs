// Golem Terminal V2 — Tab-based terminal multiplexer built on Iced + iced_term
//
// V2 uses iced_term (alacritty_terminal backend) for proper terminal rendering
// with colors, cursor, selection, scrollback. Sidebar navigation (Zen browser
// style) replaces top tab bar. pane_grid provides recursive splits.

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, column, container, mouse_area, row, rule, text, Space};
use iced::{event, keyboard, Length, Subscription, Task};
use std::sync::{Arc, Mutex};

use iced::advanced::widget::tree::Tree;
use iced::advanced::widget::Operation;
use iced::advanced::{layout, mouse, overlay, renderer};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
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
// Sidebar group layout
const SIDEBAR_SUBTITLE_SPACING: f32 = 1.0;
const SIDEBAR_STRIP_WIDTH: f32 = 2.0;
const SIDEBAR_INDENT: f32 = 8.0;
const ITERM_BACKGROUND: &str = "#1e1e1e";
const ITERM_FOREGROUND: &str = "#d4d4d4";
const CLEAR_TERMINAL_SEQUENCE: &[u8] = b"\x1b[2J\x1b[3J\x1b[H";
const FORWARD_DELETE_SEQUENCE: &[u8] = b"\x1b[3~";

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

    // Keyboard shortcuts
    KeyboardEvent(keyboard::Event),
    ClosePaneOrTab,
    ClearTerminal,
    DeleteForward,
    DeleteLineLeft,
    OptionDeleteWord,

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
        foreground: ITERM_FOREGROUND.into(),
        background: ITERM_BACKGROUND.into(),
        black: "#000000".into(),
        red: "#c91b00".into(),
        green: "#00c200".into(),
        yellow: "#c7c400".into(),
        blue: "#0225c7".into(),
        magenta: "#c930c7".into(),
        cyan: "#00c5c7".into(),
        white: "#c7c7c7".into(),
        bright_black: "#676767".into(),
        bright_red: "#ff6d67".into(),
        bright_green: "#5ff967".into(),
        bright_yellow: "#fefb67".into(),
        bright_blue: "#6871ff".into(),
        bright_magenta: "#ff76ff".into(),
        bright_cyan: "#5ffdff".into(),
        bright_white: "#feffff".into(),
        bright_foreground: None,
        dim_foreground: "#9e9e9e".into(),
        dim_black: "#0f0f0f".into(),
        dim_red: "#8b3a2f".into(),
        dim_green: "#4d7d5c".into(),
        dim_yellow: "#8e8550".into(),
        dim_blue: "#3f4f8f".into(),
        dim_magenta: "#875289".into(),
        dim_cyan: "#3f8789".into(),
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
        swallow("k", cmd),       // Cmd+K → clear terminal
        swallow("n", cmd),       // Cmd+N → new tab (alias)
        swallow("r", cmd),       // Cmd+R → clear screen
        // Backspace with modifiers (prevent iced_term from processing)
        (
            Binding {
                target: InputKind::KeyCode(iced::keyboard::key::Named::Backspace),
                modifiers: cmd,
                terminal_mode_include: iced_term::TermMode::empty(),
                terminal_mode_exclude: iced_term::TermMode::empty(),
            },
            BindingAction::Ignore,
        ),
        (
            Binding {
                target: InputKind::KeyCode(iced::keyboard::key::Named::Backspace),
                modifiers: Modifiers::ALT,
                terminal_mode_include: iced_term::TermMode::empty(),
                terminal_mode_exclude: iced_term::TermMode::empty(),
            },
            BindingAction::Ignore,
        ),
        (
            Binding {
                target: InputKind::KeyCode(iced::keyboard::key::Named::Delete),
                modifiers: cmd,
                terminal_mode_include: iced_term::TermMode::empty(),
                terminal_mode_exclude: iced_term::TermMode::empty(),
            },
            BindingAction::Ignore,
        ),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    SwitchTab(i32),
    SplitVertical,
    NewTab,
    ToggleSplit,
    ClosePaneOrTab,
    ClearTerminal,
    DeleteForward,
    DeleteLineLeft,
    DeleteWordLeft,
    ToggleSidebar,
    Quit,
    SelectTab(usize),
}

impl ShortcutAction {
    fn repeats(self) -> bool {
        matches!(self, Self::SwitchTab(_))
    }

    fn message(self) -> Message {
        match self {
            Self::SwitchTab(delta) => Message::SwitchTab(delta),
            Self::SplitVertical => Message::SplitFocused(pane_grid::Axis::Vertical),
            Self::NewTab => Message::NewTab,
            Self::ToggleSplit => Message::ToggleSplit,
            Self::ClosePaneOrTab => Message::ClosePaneOrTab,
            Self::ClearTerminal => Message::ClearTerminal,
            Self::DeleteForward => Message::DeleteForward,
            Self::DeleteLineLeft => Message::DeleteLineLeft,
            Self::DeleteWordLeft => Message::OptionDeleteWord,
            Self::ToggleSidebar => Message::ToggleSidebar,
            Self::Quit => Message::Quit,
            Self::SelectTab(idx) => Message::SelectTab(idx),
        }
    }
}

fn is_modifier_key(key: &keyboard::Key) -> bool {
    matches!(
        key,
        keyboard::Key::Named(
            keyboard::key::Named::Alt
                | keyboard::key::Named::AltGraph
                | keyboard::key::Named::Control
                | keyboard::key::Named::Shift
                | keyboard::key::Named::Meta
                | keyboard::key::Named::Super
        )
    )
}

fn key_matches(key: &keyboard::Key, expected: &str) -> bool {
    matches!(
        key,
        keyboard::Key::Character(value) if value.as_ref().eq_ignore_ascii_case(expected)
    )
}

fn event_matches_char(key: &keyboard::Key, modified_key: &keyboard::Key, expected: &str) -> bool {
    key_matches(key, expected) || key_matches(modified_key, expected)
}

fn merged_shortcut_modifiers(
    event_modifiers: keyboard::Modifiers,
    sticky_modifiers: keyboard::Modifiers,
) -> keyboard::Modifiers {
    let mut effective = event_modifiers;
    effective.insert(sticky_modifiers);
    effective
}

fn shortcut_action(
    event: &keyboard::Event,
    sticky_modifiers: keyboard::Modifiers,
) -> Option<ShortcutAction> {
    let (key, modified_key, modifiers, repeat) = match event {
        keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            repeat,
            ..
        } => (key, modified_key, *modifiers, *repeat),
        _ => return None,
    };

    let modifiers = merged_shortcut_modifiers(modifiers, sticky_modifiers);

    let action = if modifiers.command() && modifiers.alt() {
        match key {
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft)
            | keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                Some(ShortcutAction::SwitchTab(-1))
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowRight)
            | keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                Some(ShortcutAction::SwitchTab(1))
            }
            _ => None,
        }
    } else if modifiers.command() {
        if is_forward_delete_key(event) && !modifiers.alt() && !modifiers.control() {
            Some(ShortcutAction::DeleteLineLeft)
        } else {
            match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft) if !modifiers.shift() => {
                    Some(ShortcutAction::SwitchTab(-1))
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowRight) if !modifiers.shift() => {
                    Some(ShortcutAction::SwitchTab(1))
                }
                keyboard::Key::Named(keyboard::key::Named::Backspace)
                    if !modifiers.alt() && !modifiers.control() =>
                {
                    Some(ShortcutAction::DeleteLineLeft)
                }
                _ if event_matches_char(key, modified_key, "d") && modifiers.shift() => {
                    Some(ShortcutAction::SplitVertical)
                }
                _ if event_matches_char(key, modified_key, "t")
                    || event_matches_char(key, modified_key, "n") =>
                {
                    Some(ShortcutAction::NewTab)
                }
                _ if event_matches_char(key, modified_key, "d") => {
                    Some(ShortcutAction::ToggleSplit)
                }
                _ if event_matches_char(key, modified_key, "w") => {
                    Some(ShortcutAction::ClosePaneOrTab)
                }
                _ if event_matches_char(key, modified_key, "k")
                    || event_matches_char(key, modified_key, "r") =>
                {
                    Some(ShortcutAction::ClearTerminal)
                }
                _ if event_matches_char(key, modified_key, "b") => {
                    Some(ShortcutAction::ToggleSidebar)
                }
                _ if event_matches_char(key, modified_key, "q") => Some(ShortcutAction::Quit),
                _ if event_matches_char(key, modified_key, "[")
                    || event_matches_char(key, modified_key, "{") =>
                {
                    Some(ShortcutAction::SwitchTab(-1))
                }
                _ if event_matches_char(key, modified_key, "]")
                    || event_matches_char(key, modified_key, "}") =>
                {
                    Some(ShortcutAction::SwitchTab(1))
                }
                _ => {
                    let digit = match key {
                        keyboard::Key::Character(value) => value.chars().next(),
                        _ => None,
                    }
                    .and_then(|ch| ch.to_digit(10))
                    .filter(|digit| (1..=9).contains(digit));

                    digit.map(|digit| ShortcutAction::SelectTab((digit - 1) as usize))
                }
            }
        }
    } else if modifiers.alt() {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                Some(ShortcutAction::DeleteWordLeft)
            }
            _ => None,
        }
    } else if modifiers.is_empty() && is_forward_delete_key(event) {
        Some(ShortcutAction::DeleteForward)
    } else {
        None
    };

    match action {
        Some(action) if repeat && !action.repeats() => None,
        other => other,
    }
}

fn should_capture_terminal_shortcut(
    event: &keyboard::Event,
    sticky_modifiers: keyboard::Modifiers,
) -> bool {
    shortcut_action(event, sticky_modifiers).is_some()
}

fn should_listen_to_keyboard_event(event: &keyboard::Event) -> bool {
    match event {
        keyboard::Event::ModifiersChanged(_) => true,
        keyboard::Event::KeyPressed { .. } | keyboard::Event::KeyReleased { .. } => {
            let (key, modified_key) = match event {
                keyboard::Event::KeyPressed {
                    key, modified_key, ..
                }
                | keyboard::Event::KeyReleased {
                    key, modified_key, ..
                } => (key, modified_key),
                keyboard::Event::ModifiersChanged(_) => unreachable!(),
            };

            is_modifier_key(key)
                || is_forward_delete_key(event)
                || matches!(
                    key,
                    keyboard::Key::Named(
                        keyboard::key::Named::Enter
                            | keyboard::key::Named::ArrowLeft
                            | keyboard::key::Named::ArrowRight
                            | keyboard::key::Named::ArrowUp
                            | keyboard::key::Named::ArrowDown
                            | keyboard::key::Named::Backspace
                            | keyboard::key::Named::Delete
                    )
                )
                || ["[", "]", "{", "}", "b", "d", "k", "n", "q", "r", "t", "w"]
                    .iter()
                    .any(|expected| event_matches_char(key, modified_key, expected))
                || matches!(
                    key,
                    keyboard::Key::Character(value)
                        if value.chars().next().is_some_and(|ch| ('1'..='9').contains(&ch))
                )
        }
    }
}

fn is_forward_delete_key(event: &keyboard::Event) -> bool {
    match event {
        keyboard::Event::KeyPressed {
            key, physical_key, ..
        }
        | keyboard::Event::KeyReleased {
            key, physical_key, ..
        } => {
            matches!(key, keyboard::Key::Named(keyboard::key::Named::Delete))
                || matches!(
                    physical_key,
                    keyboard::key::Physical::Code(keyboard::key::Code::Delete)
                )
        }
        keyboard::Event::ModifiersChanged(_) => false,
    }
}

fn keyboard_event_message(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(event) if should_listen_to_keyboard_event(&event) => {
            Some(Message::KeyboardEvent(event))
        }
        _ => None,
    }
}

fn capture_terminal_shortcuts<'a>(
    content: impl Into<iced::Element<'a, Message>>,
    keyboard_modifiers: keyboard::Modifiers,
) -> iced::Element<'a, Message> {
    struct Capture<'a> {
        content: iced::Element<'a, Message>,
        keyboard_modifiers: keyboard::Modifiers,
    }

    impl Widget<Message, iced::Theme, iced::Renderer> for Capture<'_> {
        fn size(&self) -> iced::Size<Length> {
            self.content.as_widget().size()
        }

        fn size_hint(&self) -> iced::Size<Length> {
            self.content.as_widget().size_hint()
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(&[&self.content]);
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        }

        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            viewport: &iced::Rectangle,
        ) {
            if let iced::Event::Keyboard(key_event) = event {
                if should_capture_terminal_shortcut(key_event, self.keyboard_modifiers) {
                    shell.capture_event();
                    return;
                }
            }

            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &iced::Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            // Guard: if the cursor is outside the terminal bounds, return the
            // default interaction so the cursor icon resets when the mouse
            // moves to the sidebar or other non-terminal areas.
            if !cursor.is_over(layout.bounds()) {
                return mouse::Interaction::default();
            }
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &iced::Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &iced::Rectangle,
        ) {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        }

        fn overlay<'a>(
            &'a mut self,
            tree: &'a mut Tree,
            layout: Layout<'a>,
            renderer: &iced::Renderer,
            viewport: &iced::Rectangle,
            translation: iced::Vector,
        ) -> Option<overlay::Element<'a, Message, iced::Theme, iced::Renderer>> {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }

    iced::Element::new(Capture {
        content: content.into(),
        keyboard_modifiers,
    })
}

// ── State ────────────────────────────────────────────────────────────────────

pub struct State {
    pub slots: Vec<AgentSlot>,
    pub panes: pane_grid::State<usize>, // each pane stores a slot index
    pub focus: Option<pane_grid::Pane>,
    pub keyboard_modifiers: keyboard::Modifiers,
    pub sidebar_visible: bool,
    pub base_cmd: Vec<String>,
    pub next_slot_id: usize,
    pub test_state: Arc<Mutex<crate::test_harness::TestState>>,
    pub config: crate::config::AppConfig,
    pub collapsed_groups: std::collections::HashMap<String, bool>,
    pub agent_states: std::collections::HashMap<String, agent_state::AgentExternalState>,
    agent_state_dir: std::path::PathBuf,
    #[cfg(target_os = "macos")]
    vibrancy_state: SidebarVibrancyState,
    #[cfg(target_os = "macos")]
    vibrancy_view: Option<*mut objc2::runtime::AnyObject>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarVibrancyState {
    Pending,
    Active,
    Unsupported,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarVibrancySync {
    Deferred,
    Active,
    Unsupported,
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
            keyboard_modifiers: keyboard::Modifiers::empty(),
            sidebar_visible: true,
            base_cmd: cmd,
            next_slot_id: 1,
            test_state,
            config,
            collapsed_groups: std::collections::HashMap::new(),
            agent_states: std::collections::HashMap::new(),
            agent_state_dir: agent_state::state_dir(),
            #[cfg(target_os = "macos")]
            vibrancy_state: SidebarVibrancyState::Pending,
            #[cfg(target_os = "macos")]
            vibrancy_view: None,
        };

        state.sync_test_state();
        state
    }

    fn pane_slot_idx(&self, pane: pane_grid::Pane) -> Option<usize> {
        self.panes
            .get(pane)
            .copied()
            .filter(|&idx| idx < self.slots.len())
    }

    fn normalize_panes(&mut self) {
        if self.slots.is_empty() {
            return;
        }

        let last_valid = self.slots.len() - 1;
        let pane_updates: Vec<(pane_grid::Pane, usize)> = self
            .panes
            .iter()
            .filter_map(|(pane, &slot_idx)| (slot_idx > last_valid).then_some((*pane, last_valid)))
            .collect();

        for (pane, slot_idx) in pane_updates {
            if let Some(slot_ref) = self.panes.get_mut(pane) {
                *slot_ref = slot_idx;
            }
        }

        if self.focus.and_then(|pane| self.panes.get(pane)).is_none() {
            self.focus = self.panes.iter().next().map(|(pane, _)| *pane);
        }
    }

    /// The slot index of the focused pane, if any.
    pub fn active_slot_idx(&self) -> Option<usize> {
        self.focus.and_then(|p| self.pane_slot_idx(p))
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

    /// Returns the hex color used as the left-border accent for a group section.
    /// Uses the first golem's `color` field in the group so each category
    /// (orchestrators, workers, tools) gets a visually distinct stripe.
    /// Falls back to `config.ui.colors.accent` when the group is empty.
    pub fn group_accent_color<'a>(&'a self, group: &str) -> &'a str {
        self.golems_in_group(group)
            .into_iter()
            .next()
            .map(|g| g.color.as_str())
            .unwrap_or(&self.config.ui.colors.accent)
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
            .find(|(surface, _)| {
                surface.contains(&label_lower) || label_lower.contains(surface.as_str())
            })
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

    fn update_keyboard_modifiers(&mut self, event: &keyboard::Event) {
        match event {
            keyboard::Event::ModifiersChanged(modifiers) => {
                self.keyboard_modifiers = *modifiers;
            }
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                self.keyboard_modifiers = *modifiers;
                match key {
                    keyboard::Key::Named(keyboard::key::Named::Alt)
                    | keyboard::Key::Named(keyboard::key::Named::AltGraph) => {
                        self.keyboard_modifiers.insert(keyboard::Modifiers::ALT);
                    }
                    keyboard::Key::Named(keyboard::key::Named::Control) => {
                        self.keyboard_modifiers.insert(keyboard::Modifiers::CTRL);
                    }
                    keyboard::Key::Named(keyboard::key::Named::Shift) => {
                        self.keyboard_modifiers.insert(keyboard::Modifiers::SHIFT);
                    }
                    keyboard::Key::Named(keyboard::key::Named::Meta)
                    | keyboard::Key::Named(keyboard::key::Named::Super) => {
                        self.keyboard_modifiers.insert(keyboard::Modifiers::COMMAND);
                    }
                    _ => {}
                }
            }
            keyboard::Event::KeyReleased { key, modifiers, .. } => {
                self.keyboard_modifiers = *modifiers;
                match key {
                    keyboard::Key::Named(keyboard::key::Named::Alt)
                    | keyboard::Key::Named(keyboard::key::Named::AltGraph) => {
                        self.keyboard_modifiers.remove(keyboard::Modifiers::ALT);
                    }
                    keyboard::Key::Named(keyboard::key::Named::Control) => {
                        self.keyboard_modifiers.remove(keyboard::Modifiers::CTRL);
                    }
                    keyboard::Key::Named(keyboard::key::Named::Shift) => {
                        self.keyboard_modifiers.remove(keyboard::Modifiers::SHIFT);
                    }
                    keyboard::Key::Named(keyboard::key::Named::Meta)
                    | keyboard::Key::Named(keyboard::key::Named::Super) => {
                        self.keyboard_modifiers.remove(keyboard::Modifiers::COMMAND);
                    }
                    _ => {}
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn refresh_sidebar_vibrancy(&mut self) {
        if self.vibrancy_state == SidebarVibrancyState::Unsupported {
            return;
        }
        // Skip redundant ObjC calls when already active — ToggleSidebar and
        // ConfigReloaded reset state to Pending to force a refresh.
        if self.vibrancy_state == SidebarVibrancyState::Active {
            return;
        }

        let sidebar_w = self.config.ui.sidebar_width;
        let sidebar_visible = self.sidebar_visible;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            apply_sidebar_vibrancy(sidebar_w, sidebar_visible, &mut self.vibrancy_view)
        }));
        self.vibrancy_state = match result {
            Ok(SidebarVibrancySync::Active) => SidebarVibrancyState::Active,
            Ok(SidebarVibrancySync::Deferred) => self.vibrancy_state,
            Ok(SidebarVibrancySync::Unsupported) => SidebarVibrancyState::Unsupported,
            Err(_) => {
                eprintln!("[vibrancy] panic during init — disabling vibrancy");
                SidebarVibrancyState::Unsupported
            }
        };
    }

    // ── Update ───────────────────────────────────────────────────────────────

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Apply sidebar vibrancy on first update — window exists, on main thread.
        // We insert/update an NSVisualEffectView behind the Metal layer covering
        // only the sidebar area. win.transparent=true is required so the Metal
        // layer renders transparent pixels in the sidebar, letting the native
        // vibrancy view show through.
        //
        // SAFETY: this runs inside the winit event handler which is an ObjC block
        // (extern "C"). Any Rust panic here becomes panic_cannot_unwind → abort().
        // catch_unwind ensures that ObjC/class-lookup failures disable vibrancy
        // gracefully instead of crashing the process.
        #[cfg(target_os = "macos")]
        self.refresh_sidebar_vibrancy();

        let task = match message {
            Message::KeyboardEvent(event) => {
                let sticky_modifiers = self.keyboard_modifiers;

                if let Some(action) = shortcut_action(&event, sticky_modifiers) {
                    self.update_keyboard_modifiers(&event);
                    let repeats = matches!(event, keyboard::Event::KeyPressed { repeat: true, .. });
                    if !repeats || action.repeats() {
                        return self.update(action.message());
                    }
                }

                if let keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Enter),
                    modifiers,
                    repeat,
                    ..
                } = &event
                {
                    let modifiers = merged_shortcut_modifiers(*modifiers, sticky_modifiers);
                    if !repeat && modifiers.is_empty() {
                        if let Some(slot_idx) = self.active_slot_idx() {
                            if self
                                .slots
                                .get(slot_idx)
                                .is_some_and(|slot| slot.status == SlotStatus::Idle)
                            {
                                return self.update(Message::LaunchSlot(slot_idx));
                            }
                        }
                    }
                }

                self.update_keyboard_modifiers(&event);
                Task::none()
            }

            Message::LaunchSlot(idx) => {
                let widget_id = if let Some(slot) = self.slots.get_mut(idx) {
                    if slot.status != SlotStatus::Idle {
                        return Task::none();
                    }
                    slot.launch();
                    slot.terminal.as_ref().map(|term| term.widget_id().clone())
                } else {
                    None
                };
                if let Some(wid) = widget_id {
                    self.sync_test_state();
                    return TerminalView::focus(wid);
                }
                Task::none()
            }

            Message::KillSlot(idx) => {
                if let Some(slot) = self.slots.get_mut(idx) {
                    slot.kill();
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
                if self.slots.get(idx).is_some() {
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
                    if let Some(term) = self.slots.get(idx).and_then(|slot| slot.terminal.as_ref())
                    {
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
                if self.slots.len() <= 1 || idx >= self.slots.len() {
                    return Task::none();
                }
                if let Some(slot) = self.slots.get_mut(idx) {
                    slot.kill();
                }
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

                self.normalize_panes();

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
                            if let Some(term) = self
                                .pane_slot_idx(adj)
                                .and_then(|slot_idx| self.slots.get(slot_idx))
                                .and_then(|slot| slot.terminal.as_ref())
                            {
                                self.sync_test_state();
                                return TerminalView::focus(term.widget_id().clone());
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
                if let Some(term) = self
                    .slots
                    .get(new_idx)
                    .and_then(|slot| slot.terminal.as_ref())
                {
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
                    if let Some(term) = self
                        .focus
                        .and_then(|pane| self.pane_slot_idx(pane))
                        .and_then(|slot_idx| self.slots.get(slot_idx))
                        .and_then(|slot| slot.terminal.as_ref())
                    {
                        self.sync_test_state();
                        return TerminalView::focus(term.widget_id().clone());
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
                        if let Some(term) = self
                            .slots
                            .get(secondary)
                            .and_then(|slot| slot.terminal.as_ref())
                        {
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
                            self.normalize_panes();
                            if let Some(term) = self
                                .pane_slot_idx(sibling)
                                .and_then(|slot_idx| self.slots.get(slot_idx))
                                .and_then(|slot| slot.terminal.as_ref())
                            {
                                self.sync_test_state();
                                return TerminalView::focus(term.widget_id().clone());
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::ClosePaneOrTab => {
                if self.panes.len() > 1 {
                    // Split mode: close the focused pane
                    if let Some(focused) = self.focus {
                        if let Some((_, sibling)) = self.panes.close(focused) {
                            self.focus = Some(sibling);
                            self.normalize_panes();
                            if let Some(term) = self
                                .pane_slot_idx(sibling)
                                .and_then(|slot_idx| self.slots.get(slot_idx))
                                .and_then(|slot| slot.terminal.as_ref())
                            {
                                self.sync_test_state();
                                return TerminalView::focus(term.widget_id().clone());
                            }
                        }
                    }
                } else {
                    // Single pane: close the active tab
                    if let Some(idx) = self.active_slot_idx() {
                        if self.slots.len() > 1 {
                            return self.update(Message::CloseTab(idx));
                        }
                        // Last tab: quit
                        return self.update(Message::Quit);
                    }
                }
                Task::none()
            }

            Message::ClearTerminal => {
                // Send clear screen escape sequence to focused terminal
                if let Some(slot_idx) = self.active_slot_idx() {
                    if let Some(slot) = self.slots.get_mut(slot_idx) {
                        if let Some(ref mut terminal) = slot.terminal {
                            let cmd = iced_term::Command::ProxyToBackend(
                                iced_term::backend::Command::Write(
                                    CLEAR_TERMINAL_SEQUENCE.to_vec(),
                                ),
                            );
                            terminal.handle(cmd);
                        }
                    }
                }
                Task::none()
            }

            Message::DeleteForward => {
                if let Some(slot_idx) = self.active_slot_idx() {
                    if let Some(slot) = self.slots.get_mut(slot_idx) {
                        if let Some(ref mut terminal) = slot.terminal {
                            let cmd = iced_term::Command::ProxyToBackend(
                                iced_term::backend::Command::Write(
                                    FORWARD_DELETE_SEQUENCE.to_vec(),
                                ),
                            );
                            terminal.handle(cmd);
                        }
                    }
                }
                Task::none()
            }

            Message::DeleteLineLeft => {
                // Cmd+Backspace matches iTerm's "delete to line start" mapping.
                if let Some(slot_idx) = self.active_slot_idx() {
                    if let Some(slot) = self.slots.get_mut(slot_idx) {
                        if let Some(ref mut terminal) = slot.terminal {
                            let cmd = iced_term::Command::ProxyToBackend(
                                iced_term::backend::Command::Write(vec![0x15]), // Ctrl+U
                            );
                            terminal.handle(cmd);
                        }
                    }
                }
                Task::none()
            }

            Message::OptionDeleteWord => {
                // Send ESC+DEL (Option+Backspace word delete) to focused terminal
                if let Some(slot_idx) = self.active_slot_idx() {
                    if let Some(slot) = self.slots.get_mut(slot_idx) {
                        if let Some(ref mut terminal) = slot.terminal {
                            let cmd = iced_term::Command::ProxyToBackend(
                                iced_term::backend::Command::Write(vec![0x1b, 0x7f]),
                            );
                            terminal.handle(cmd);
                        }
                    }
                }
                Task::none()
            }

            Message::PaneClicked(pane) => {
                self.focus = Some(pane);
                if let Some(slot_idx) = self.pane_slot_idx(pane) {
                    // If terminal is running, focus it
                    if let Some(term) = self
                        .slots
                        .get(slot_idx)
                        .and_then(|slot| slot.terminal.as_ref())
                    {
                        self.sync_test_state();
                        return TerminalView::focus(term.widget_id().clone());
                    }
                    // If idle, launch it
                    let launch_wid = if let Some(slot) = self.slots.get_mut(slot_idx) {
                        if slot.status == SlotStatus::Idle {
                            slot.launch();
                            slot.terminal.as_ref().map(|term| term.widget_id().clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(wid) = launch_wid {
                        self.sync_test_state();
                        return TerminalView::focus(wid);
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
                #[cfg(target_os = "macos")]
                {
                    self.vibrancy_state = SidebarVibrancyState::Pending;
                    self.refresh_sidebar_vibrancy();
                }
                // Re-focus current terminal
                if let Some(term) = self
                    .active_slot_idx()
                    .and_then(|slot_idx| self.slots.get(slot_idx))
                    .and_then(|slot| slot.terminal.as_ref())
                {
                    self.sync_test_state();
                    return TerminalView::focus(term.widget_id().clone());
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
                #[cfg(target_os = "macos")]
                {
                    self.vibrancy_state = SidebarVibrancyState::Pending;
                    self.refresh_sidebar_vibrancy();
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

        subscriptions.push(event::listen_with(keyboard_event_message));

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
        // Opaque background for the terminal area — prevents grey bleed when
        // win.transparent=true is used for sidebar vibrancy.
        let terminal_bg = parse_hex_color(ITERM_BACKGROUND);

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
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(terminal_bg)),
                ..Default::default()
            })
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
        let terminal_bg = crate::config::parse_hex_color(ITERM_BACKGROUND);
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
            container(capture_terminal_shortcuts(
                TerminalView::show(term).map(Message::TermEvent),
                self.keyboard_modifiers,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(terminal_bg)),
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
                background: Some(iced::Background::Color(terminal_bg)),
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
        // When vibrancy is active the Iced sidebar container must be transparent
        // so the NSVisualEffectView behind the Metal layer shows through.
        #[cfg(target_os = "macos")]
        let sidebar_transparent = self.vibrancy_state == SidebarVibrancyState::Active;
        #[cfg(not(target_os = "macos"))]
        let sidebar_transparent = false;
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
            // Render grouped golem presets.
            // Each group is wrapped in a row with a 2px colored accent strip on
            // the left so categories (Orchestrators / Workers / Tools) are visually
            // distinct. The accent color comes from the first golem in the group.
            for group_name in &groups {
                let collapsed = self
                    .collapsed_groups
                    .get(group_name)
                    .copied()
                    .unwrap_or(false);
                let arrow = if collapsed { ">" } else { "v" };
                let header_label = format!("{} {}", arrow, group_name.to_uppercase());

                // Accent color: first golem's color, or config accent as fallback.
                let group_color = parse_hex_color(self.group_accent_color(group_name));

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

                // Build the group sub-column: header + golem items (when expanded).
                let mut group_col = column![group_header].spacing(0.0);

                if !collapsed {
                    let golems = self.golems_in_group(group_name);
                    for golem in golems {
                        let golem_color = parse_hex_color(&golem.color);
                        let golem_name = golem.name.clone();

                        // Find the best slot for this golem: prefer active > visible > first match.
                        let running_slot = {
                            let matches: Vec<(usize, &AgentSlot)> = self
                                .slots
                                .iter()
                                .enumerate()
                                .filter(|(_, s)| s.label == golem.name)
                                .collect();
                            matches
                                .iter()
                                .find(|(idx, _)| active_slot == Some(*idx))
                                .or_else(|| {
                                    matches.iter().find(|(idx, _)| visible_slots.contains(idx))
                                })
                                .or_else(|| matches.first())
                                .copied()
                        };

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
                                "idle" => status_idle,
                                _ => dot_color,
                            }
                        } else {
                            dot_color
                        };

                        let icon_text = text(&golem.icon).size(font_tab);
                        let label_widget = text(&golem.name).size(font_tab).color(label_color);
                        let dot = text("●").size(STATUS_DOT_SIZE).color(dot_color);

                        let top_row = row![
                            icon_text,
                            label_widget,
                            Space::new().width(Length::Fill),
                            dot
                        ]
                        .spacing(SPACING_TIGHT)
                        .align_y(iced::Alignment::Center);

                        // Build tab content: top row + optional subtitle from agent state
                        let tab_content: iced::Element<'_, Message> = if let Some(ext) = ext_state {
                            let summary = ext.sidebar_summary();
                            if summary.is_empty() {
                                top_row.into()
                            } else {
                                let subtitle = text(summary).size(font_tiny).color(text_secondary);
                                column![top_row, subtitle]
                                    .spacing(SIDEBAR_SUBTITLE_SPACING)
                                    .into()
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
                                    width: if is_bordered {
                                        SIDEBAR_STRIP_WIDTH
                                    } else {
                                        0.0
                                    },
                                    radius: iced::border::radius(0.0)
                                        .top_right(BORDER_RADIUS)
                                        .bottom_right(BORDER_RADIUS),
                                },
                                ..Default::default()
                            })
                            .padding([SPACING_TIGHT, SPACING_NORMAL + SIDEBAR_INDENT]) // indent under group
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

                        group_col = group_col.push(tab_element);
                    }
                }

                // Accent strip on the left edge of the group section.
                let accent_strip = container(Space::new())
                    .width(SIDEBAR_STRIP_WIDTH)
                    .height(Length::Fill)
                    .style(move |_theme| container::Style {
                        background: Some(iced::Background::Color(group_color)),
                        ..Default::default()
                    });

                let group_row = row![accent_strip, group_col.width(Length::Fill)]
                    .align_y(iced::Alignment::Start);
                sidebar_content = sidebar_content.push(group_row);
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
                background: if sidebar_transparent {
                    None // transparent: NSVisualEffectView composites behind Metal layer
                } else {
                    Some(iced::Background::Color(bg_sidebar))
                },
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

// ── macOS Sidebar Vibrancy ───────────────────────────────────────────────────
//
// Strategy: Keep winit's WinitView as NSWindow.contentView and insert an
// NSVisualEffectView as a sibling underneath it in the frame view. Because
// win.transparent=true is set, Iced renders transparent pixels wherever the
// sidebar container has background=None, allowing the NSVisualEffectView to
// show through while the terminal area remains opaque.

/// Returns the macOS major version (e.g. 15 for Sequoia), or 0 on failure.
#[cfg(target_os = "macos")]
fn macos_major_version() -> u32 {
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    let ret = unsafe {
        libc::sysctlbyname(
            b"kern.osproductversion\0".as_ptr() as *const libc::c_char,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return 0;
    }
    // The string is NUL-terminated; trim the NUL before parsing.
    let s = std::str::from_utf8(&buf[..len.saturating_sub(1)]).unwrap_or("0");
    s.split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0)
}

// NSVisualEffectMaterial.sidebar = 7 (macOS 10.14+)
const NS_VISUAL_EFFECT_MATERIAL_SIDEBAR: i64 = 7;
// NSVisualEffectState.active = 1
const NS_VISUAL_EFFECT_STATE_ACTIVE: i64 = 1;
// NSVisualEffectBlendingMode.behindWindow = 0
const NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW: i64 = 0;

/// Insert an NSVisualEffectView covering `sidebar_width` pixels on the left
/// behind Iced's Metal layer. Keeps WinitView as `contentView`.
///
/// Requires macOS 15+ (Sequoia). Returns `Deferred` until a live window exists.
#[cfg(target_os = "macos")]
fn apply_sidebar_vibrancy(
    sidebar_width: f32,
    sidebar_visible: bool,
    vibrancy_view: &mut Option<*mut objc2::runtime::AnyObject>,
) -> SidebarVibrancySync {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSAutoresizingMaskOptions};
    use objc2_foundation::{NSRect, NSSize};

    let major = macos_major_version();
    if major < 15 {
        eprintln!("[vibrancy] macOS 15+ required (found {major}.x), skipping");
        return SidebarVibrancySync::Unsupported;
    }

    let Some(mtm) = MainThreadMarker::new() else {
        return SidebarVibrancySync::Deferred;
    };

    let mut result = SidebarVibrancySync::Deferred;

    autoreleasepool(|_pool| {
        let app = NSApplication::sharedApplication(mtm);

        let window = app.keyWindow().or_else(|| app.mainWindow());
        let Some(window) = window else {
            return;
        };
        let Some(content_view) = window.contentView() else {
            return;
        };
        let Some(frame_view) = (unsafe { content_view.superview() }) else {
            return;
        };

        const NS_WINDOW_BELOW: isize = -1;

        let content_frame = content_view.frame();
        let sidebar_frame = NSRect {
            origin: content_frame.origin,
            size: NSSize {
                width: sidebar_width.max(0.0) as f64,
                height: content_frame.size.height,
            },
        };
        let autoresizing_mask = NSAutoresizingMaskOptions::ViewHeightSizable
            | NSAutoresizingMaskOptions::ViewMaxXMargin;
        let vev_hidden = !sidebar_visible || sidebar_width <= 0.0;

        if let Some(existing_vev) = *vibrancy_view {
            let _: () = unsafe { msg_send![existing_vev, setFrame: sidebar_frame] };
            let _: () = unsafe { msg_send![existing_vev, setAutoresizingMask: autoresizing_mask] };
            let _: () = unsafe { msg_send![existing_vev, setHidden: vev_hidden] };
            result = SidebarVibrancySync::Active;
            return;
        }

        let vev_cls = class!(NSVisualEffectView);
        let vev_alloc: *mut AnyObject = unsafe { msg_send![vev_cls, alloc] };
        let vev: *mut AnyObject = unsafe { msg_send![vev_alloc, initWithFrame: sidebar_frame] };
        if vev.is_null() {
            eprintln!("[vibrancy] NSVisualEffectView init failed");
            result = SidebarVibrancySync::Unsupported;
            return;
        }

        let _: () = unsafe { msg_send![vev, setMaterial: NS_VISUAL_EFFECT_MATERIAL_SIDEBAR] };
        let _: () = unsafe { msg_send![vev, setState: NS_VISUAL_EFFECT_STATE_ACTIVE] };
        let _: () =
            unsafe { msg_send![vev, setBlendingMode: NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW] };
        let _: () = unsafe { msg_send![vev, setAutoresizingMask: autoresizing_mask] };
        let _: () = unsafe { msg_send![vev, setHidden: vev_hidden] };
        let _: () = unsafe {
            msg_send![&*frame_view, addSubview: vev, positioned: NS_WINDOW_BELOW, relativeTo: Some(&*content_view)]
        };
        *vibrancy_view = Some(vev);
        let _: () = unsafe { msg_send![vev, release] };

        window.setInitialFirstResponder(Some(&content_view));
        let _ = window.makeFirstResponder(Some(&content_view));

        eprintln!(
            "[vibrancy] installed sibling VEV under WinitView ({sidebar_width}px x {})",
            content_frame.size.height
        );
        result = SidebarVibrancySync::Active;
    });

    result
}

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
        // Required for sidebar vibrancy: transparent Metal pixels in the sidebar
        // area let the NSVisualEffectView show through.
        win.transparent = true;
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
    .theme(|state: &State| {
        #[cfg(target_os = "macos")]
        if state.vibrancy_state == SidebarVibrancyState::Active {
            // Use a custom Dark theme with transparent background so the Metal
            // clear pass leaves alpha=0 pixels where no widget draws.  The
            // NSVisualEffectView behind the Metal layer shows through those
            // transparent regions (the sidebar).  Terminal panes and content
            // area draw their own opaque backgrounds.
            let mut custom = iced::Theme::Dark.palette();
            custom.background = iced::Color::TRANSPARENT;
            return iced::Theme::custom("Dark Vibrancy".to_string(), custom);
        }
        let _ = state; // suppress unused warning on non-macOS
        iced::Theme::Dark
    })
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

    fn key_pressed(
        key: keyboard::Key,
        modified_key: keyboard::Key,
        physical_key: keyboard::key::Physical,
        modifiers: keyboard::Modifiers,
    ) -> keyboard::Event {
        keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }
    }

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
    fn close_tab_ignores_stale_index() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::NewTab);
        let _ = state.update(Message::CloseTab(99));
        assert_eq!(state.slots.len(), 2);
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
    fn active_slot_idx_ignores_stale_pane_slot_reference() {
        let mut state = State::new(vec!["echo".into()]);
        if let Some(focused) = state.focus {
            if let Some(slot_ref) = state.panes.get_mut(focused) {
                *slot_ref = 99;
            }
        }
        assert_eq!(state.active_slot_idx(), None);
        state.normalize_panes();
        assert_eq!(state.active_slot_idx(), Some(0));
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

    // ── Phase E: Group Accent Color Tests ────────────────────────────────────

    #[test]
    fn group_accent_color_uses_first_golem_color() {
        let state = state_with_golems();
        // Each canonical group's accent = the first golem's color in that group.
        assert_eq!(state.group_accent_color("orchestrators"), "#7C3AED");
        assert_eq!(state.group_accent_color("workers"), "#06B6D4");
        assert_eq!(state.group_accent_color("tools"), "#6B7280");
    }

    #[test]
    fn group_accent_color_falls_back_to_config_accent_for_empty_group() {
        let mut state = State::new(vec!["echo".into()]);
        // Register a group with no matching golems → should fall back to accent.
        state
            .config
            .groups
            .insert("empty".into(), vec!["nobody".into()]);
        let accent = state.config.ui.colors.accent.clone();
        assert_eq!(state.group_accent_color("empty"), accent.as_str());
        // Completely unknown group (not in config.groups) → also falls back to accent.
        assert_eq!(
            state.group_accent_color("nonexistent_group_name"),
            accent.as_str()
        );
    }

    #[test]
    fn close_pane_or_tab_in_single_pane_closes_tab() {
        let mut state = State::new(vec!["echo".into()]);
        // Add a second tab
        let _ = state.update(Message::NewTab);
        assert_eq!(state.slots.len(), 2);
        // Select tab 1
        let _ = state.update(Message::SelectTab(1));
        // ClosePaneOrTab in single-pane mode should close the tab
        let _ = state.update(Message::ClosePaneOrTab);
        assert_eq!(
            state.slots.len(),
            1,
            "tab should be removed in single-pane mode"
        );
    }

    #[test]
    fn close_pane_or_tab_in_split_closes_pane() {
        let mut state = State::new(vec!["echo".into()]);
        let _ = state.update(Message::SplitFocused(pane_grid::Axis::Horizontal));
        assert_eq!(state.panes.len(), 2, "should have 2 panes after split");
        let _ = state.update(Message::ClosePaneOrTab);
        assert_eq!(state.panes.len(), 1, "should close pane, not tab");
    }

    #[test]
    fn pane_clicked_with_stale_slot_index_is_noop() {
        let mut state = State::new(vec!["echo".into()]);
        let pane = state.focus.unwrap();
        if let Some(slot_ref) = state.panes.get_mut(pane) {
            *slot_ref = 99;
        }
        let _ = state.update(Message::PaneClicked(pane));
        assert_eq!(state.focus, Some(pane));
        assert_eq!(state.active_slot_idx(), None);
    }

    #[test]
    fn shortcut_action_uses_sticky_command_modifiers() {
        let event = key_pressed(
            keyboard::Key::Character("k".into()),
            keyboard::Key::Character("k".into()),
            keyboard::key::Physical::Code(keyboard::key::Code::KeyK),
            keyboard::Modifiers::empty(),
        );

        assert_eq!(
            shortcut_action(&event, keyboard::Modifiers::COMMAND),
            Some(ShortcutAction::ClearTerminal)
        );
    }

    #[test]
    fn keyboard_event_enter_launches_idle_active_slot() {
        let mut state = State::new(vec!["echo".into()]);
        let enter = key_pressed(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::key::Physical::Code(keyboard::key::Code::Enter),
            keyboard::Modifiers::empty(),
        );

        let _ = state.update(Message::KeyboardEvent(enter));

        assert_eq!(state.slots[0].status, SlotStatus::Running);
    }

    #[test]
    fn shortcut_action_maps_iterm_tab_navigation_variants() {
        let cmd_left = key_pressed(
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            keyboard::key::Physical::Code(keyboard::key::Code::ArrowLeft),
            keyboard::Modifiers::COMMAND,
        );
        let cmd_shift_bracket = key_pressed(
            keyboard::Key::Character("[".into()),
            keyboard::Key::Character("{".into()),
            keyboard::key::Physical::Code(keyboard::key::Code::BracketLeft),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
        );

        assert_eq!(
            shortcut_action(&cmd_left, keyboard::Modifiers::empty()),
            Some(ShortcutAction::SwitchTab(-1))
        );
        assert_eq!(
            shortcut_action(&cmd_shift_bracket, keyboard::Modifiers::empty()),
            Some(ShortcutAction::SwitchTab(-1))
        );
    }

    #[test]
    fn shortcut_action_maps_physical_forward_delete() {
        let delete = key_pressed(
            keyboard::Key::Character(" ".into()),
            keyboard::Key::Character(" ".into()),
            keyboard::key::Physical::Code(keyboard::key::Code::Delete),
            keyboard::Modifiers::empty(),
        );

        assert!(should_listen_to_keyboard_event(&delete));
        assert_eq!(
            shortcut_action(&delete, keyboard::Modifiers::empty()),
            Some(ShortcutAction::DeleteForward)
        );
    }

    #[test]
    fn shortcut_action_maps_command_forward_delete_to_delete_line_left() {
        let cmd_delete = key_pressed(
            keyboard::Key::Character(" ".into()),
            keyboard::Key::Character(" ".into()),
            keyboard::key::Physical::Code(keyboard::key::Code::Delete),
            keyboard::Modifiers::COMMAND,
        );

        assert_eq!(
            shortcut_action(&cmd_delete, keyboard::Modifiers::empty()),
            Some(ShortcutAction::DeleteLineLeft)
        );
    }
}
