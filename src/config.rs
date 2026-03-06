// Golem Terminal configuration — golems.toml
//
// Layered config via Figment: defaults → file → env → CLI.
// Hot-reload via notify crate (as Iced subscription when gui feature is active).

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Config Path ──────────────────────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("golem-terminal")
}

pub fn config_path() -> PathBuf {
    config_dir().join("golems.toml")
}

// ── Config Structs ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GolemConfig {
    pub name: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_golem_type", rename = "type")]
    pub golem_type: GolemType,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub context_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GolemType {
    Orchestrator,
    Worker,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default)]
    pub golem: Vec<GolemConfig>,
    #[serde(default)]
    pub groups: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub shell: ShellConfig,
}

// ── UI Config ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "default_bottom_bar_height")]
    pub bottom_bar_height: f32,
    #[serde(default = "default_pane_spacing")]
    pub pane_spacing: f32,
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub colors: ColorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontConfig {
    #[serde(default = "default_font_small")]
    pub small: f32,
    #[serde(default = "default_font_tiny")]
    pub tiny: f32,
    #[serde(default = "default_font_tab")]
    pub tab: f32,
    #[serde(default = "default_font_group")]
    pub group: f32,
    #[serde(default = "default_font_terminal")]
    pub terminal: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColorConfig {
    #[serde(default = "default_bg_primary")]
    pub bg_primary: String,
    #[serde(default = "default_bg_secondary")]
    pub bg_secondary: String,
    #[serde(default = "default_bg_sidebar")]
    pub bg_sidebar: String,
    #[serde(default = "default_bg_tab_active")]
    pub bg_tab_active: String,
    #[serde(default = "default_bg_tab_hover")]
    pub bg_tab_hover: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_text_secondary")]
    pub text_secondary: String,
    #[serde(default = "default_text_tab_active")]
    pub text_tab_active: String,
    #[serde(default = "default_status_running")]
    pub status_running: String,
    #[serde(default = "default_status_pending")]
    pub status_pending: String,
    #[serde(default = "default_status_idle")]
    pub status_idle: String,
    #[serde(default = "default_focus_border")]
    pub focus_border: String,
}

// ── Shell Config ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShellConfig {
    #[serde(default = "default_shell_program")]
    pub program: String,
    #[serde(default = "default_shell_args")]
    pub args: Vec<String>,
}

// ── Defaults ─────────────────────────────────────────────────────────────────

fn default_icon() -> String {
    "●".into()
}

fn default_color() -> String {
    "#6B7280".into()
}

fn default_golem_type() -> GolemType {
    GolemType::Worker
}

// UI layout defaults
fn default_sidebar_width() -> f32 { 200.0 }
fn default_bottom_bar_height() -> f32 { 24.0 }
fn default_pane_spacing() -> f32 { 2.0 }

// Font defaults
fn default_font_small() -> f32 { 12.0 }
fn default_font_tiny() -> f32 { 10.0 }
fn default_font_tab() -> f32 { 13.0 }
fn default_font_group() -> f32 { 11.0 }
fn default_font_terminal() -> f32 { 14.0 }

// Color defaults (hex strings matching current hardcoded iced::Color values)
fn default_bg_primary() -> String { "#1c1c24".into() }
fn default_bg_secondary() -> String { "#262631".into() }
fn default_bg_sidebar() -> String { "#1f1f26".into() }
fn default_bg_tab_active() -> String { "#2e2e38".into() }
fn default_bg_tab_hover() -> String { "#292933".into() }
fn default_accent() -> String { "#6699f2".into() }
fn default_text_secondary() -> String { "#8c8c99".into() }
fn default_text_tab_active() -> String { "#f2f2f2".into() }
fn default_status_running() -> String { "#4dcc66".into() }
fn default_status_pending() -> String { "#e6b333".into() }
fn default_status_idle() -> String { "#737380".into() }
fn default_focus_border() -> String { "#598ce6".into() }

// Shell defaults
fn default_shell_program() -> String { "/bin/zsh".into() }
fn default_shell_args() -> Vec<String> { vec!["-l".into()] }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            golem: vec![],
            groups: HashMap::new(),
            ui: UiConfig::default(),
            shell: ShellConfig::default(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            sidebar_width: default_sidebar_width(),
            bottom_bar_height: default_bottom_bar_height(),
            pane_spacing: default_pane_spacing(),
            font: FontConfig::default(),
            colors: ColorConfig::default(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            small: default_font_small(),
            tiny: default_font_tiny(),
            tab: default_font_tab(),
            group: default_font_group(),
            terminal: default_font_terminal(),
        }
    }
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            bg_primary: default_bg_primary(),
            bg_secondary: default_bg_secondary(),
            bg_sidebar: default_bg_sidebar(),
            bg_tab_active: default_bg_tab_active(),
            bg_tab_hover: default_bg_tab_hover(),
            accent: default_accent(),
            text_secondary: default_text_secondary(),
            text_tab_active: default_text_tab_active(),
            status_running: default_status_running(),
            status_pending: default_status_pending(),
            status_idle: default_status_idle(),
            focus_border: default_focus_border(),
        }
    }
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: default_shell_program(),
            args: default_shell_args(),
        }
    }
}

// ── Hex Color Parsing ───────────────────────────────────────────────────────

/// Parse a hex color string (e.g. "#1b1b1f" or "1b1b1f") into an iced::Color.
/// Falls back to magenta on invalid input for easy visual debugging.
#[cfg(feature = "gui")]
pub fn parse_hex_color(hex: &str) -> iced::Color {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return iced::Color::from_rgb(1.0, 0.0, 1.0); // magenta = broken
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    iced::Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// Parse hex color with custom alpha (for sidebar vibrancy).
#[cfg(feature = "gui")]
pub fn parse_hex_color_alpha(hex: &str, alpha: f32) -> iced::Color {
    let mut c = parse_hex_color(hex);
    c.a = alpha;
    c
}

// ── Load Config ──────────────────────────────────────────────────────────────

/// Load config with Figment provider chain: defaults → file → env.
pub fn load() -> Result<AppConfig, figment::Error> {
    let path = config_path();

    let mut figment = Figment::from(Serialized::defaults(AppConfig::default()));

    if path.exists() {
        figment = figment.merge(Toml::file(&path));
    }

    // Env vars: GOLEM_TERMINAL_ prefix, e.g. GOLEM_TERMINAL_GROUPS_WORKERS="a,b"
    figment = figment.merge(Env::prefixed("GOLEM_TERMINAL_").split("_"));

    figment.extract()
}

/// Expand ~ in repo paths to the user's home directory.
pub fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Create the default config file if it doesn't exist.
pub fn ensure_default_config() {
    let path = config_path();
    if path.exists() {
        return;
    }

    // Create config directory
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let default_toml = r##"# Golem Terminal Configuration
# Each [[golem]] defines an agent preset shown in the sidebar.
# Groups organize golems into collapsible sections.

# ── Golem Presets ─────────────────────────────

# [[golem]]
# name = "orcClaude"
# repo = "~/Gits/orchestrator"
# icon = "🎭"
# color = "#7C3AED"
# type = "orchestrator"
# command = ["claude", "-s"]

# [[golem]]
# name = "brainClaude"
# repo = "~/Gits/brainlayer"
# icon = "🧠"
# color = "#06B6D4"
# type = "worker"
# command = ["claude", "-s"]

# [[golem]]
# name = "golemsClaude"
# repo = "~/Gits/golems"
# icon = "📦"
# color = "#F59E0B"
# type = "worker"
# command = ["claude", "-s"]

# ── Groups ────────────────────────────────────

# [groups]
# orchestrators = ["orcClaude"]
# workers = ["brainClaude", "golemsClaude"]

# ── Shell ──────────────────────────────────────────
# [shell]
# program = "/bin/zsh"
# args = ["-l"]

# ── UI Theme ──────────────────────────────────────
# [ui]
# sidebar_width = 200.0
# bottom_bar_height = 24.0
# pane_spacing = 2.0
#
# [ui.font]
# small = 12.0
# tiny = 10.0
# tab = 13.0
# group = 11.0
# terminal = 14.0
#
# [ui.colors]
# bg_primary = "#1c1c24"
# bg_secondary = "#262631"
# bg_sidebar = "#1f1f26"
# bg_tab_active = "#2e2e38"
# bg_tab_hover = "#292933"
# accent = "#6699f2"
# text_secondary = "#8c8c99"
# text_tab_active = "#f2f2f2"
# status_running = "#4dcc66"
# status_pending = "#e6b333"
# status_idle = "#737380"
# focus_border = "#598ce6"
"##;

    let _ = std::fs::write(&path, default_toml);
}

// ── Hot-Reload Subscription (GUI only) ───────────────────────────────────────

#[cfg(feature = "gui")]
pub fn watch_config() -> iced::Subscription<AppConfig> {
    use iced::stream;
    use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    iced::Subscription::run(|| {
        stream::channel(8, move |mut sender: futures::channel::mpsc::Sender<AppConfig>| async move {
            use futures::sink::SinkExt;

            let path = config_path();
            let watch_dir = config_dir();

            // Create directory if needed
            let _ = std::fs::create_dir_all(&watch_dir);

            let (tx, rx) = std::sync::mpsc::channel();

            let mut watcher: RecommendedWatcher =
                match notify::Watcher::new(tx, notify::Config::default()) {
                    Ok(w) => w,
                    Err(e) => {
                        eprintln!("[config] failed to create watcher: {e}");
                        // Block forever — no watcher, no updates
                        futures::future::pending::<()>().await;
                        return;
                    }
                };

            if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
                eprintln!("[config] failed to watch {}: {e}", watch_dir.display());
                futures::future::pending::<()>().await;
                return;
            }

            eprintln!("[config] watching {}", path.display());

            // Move to blocking thread for the mpsc recv loop
            let (done_tx, done_rx) = futures::channel::oneshot::channel::<()>();
            std::thread::spawn(move || {
                while let Ok(result) = rx.recv() {
                    match result {
                        Ok(Event {
                            kind: EventKind::Modify(_) | EventKind::Create(_),
                            paths,
                            ..
                        }) => {
                            // Only reload if our config file was modified
                            if paths.iter().any(|p| p.ends_with("golems.toml")) {
                                match load() {
                                    Ok(config) => {
                                        eprintln!("[config] reloaded golems.toml");
                                        let _ = futures::executor::block_on(
                                            sender.send(config),
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("[config] reload error: {e}");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[config] watch error: {e}");
                        }
                        _ => {}
                    }
                }
                let _ = done_tx.send(());
            });

            let _ = done_rx.await;
        })
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_empty() {
        let config = AppConfig::default();
        assert!(config.golem.is_empty());
        assert!(config.groups.is_empty());
    }

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
[[golem]]
name = "test"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.golem.len(), 1);
        assert_eq!(config.golem[0].name, "test");
        assert_eq!(config.golem[0].golem_type, GolemType::Worker); // default
        assert_eq!(config.golem[0].icon, "●"); // default
    }

    #[test]
    fn parse_full_config() {
        let toml_str = r##"
[[golem]]
name = "orcClaude"
repo = "~/Gits/orchestrator"
icon = "🎭"
color = "#7C3AED"
type = "orchestrator"
command = ["claude", "code"]

[[golem]]
name = "brainClaude"
repo = "~/Gits/brainlayer"
icon = "🧠"
color = "#06B6D4"
type = "worker"

[groups]
orchestrators = ["orcClaude"]
workers = ["brainClaude"]
"##;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.golem.len(), 2);
        assert_eq!(config.golem[0].golem_type, GolemType::Orchestrator);
        assert_eq!(config.golem[1].golem_type, GolemType::Worker);
        assert_eq!(config.groups.len(), 2);
        assert_eq!(config.groups["orchestrators"], vec!["orcClaude"]);
    }

    #[test]
    fn expand_tilde_path() {
        let expanded = expand_path("~/Gits/test");
        assert!(!expanded.to_str().unwrap().starts_with("~/"));
    }

    #[test]
    fn expand_absolute_path_unchanged() {
        let expanded = expand_path("/usr/local/bin");
        assert_eq!(expanded, PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn figment_defaults_work() {
        let config = Figment::from(Serialized::defaults(AppConfig::default()))
            .extract::<AppConfig>()
            .unwrap();
        assert!(config.golem.is_empty());
    }

    #[test]
    fn figment_merges_toml_string() {
        use figment::providers::Toml;
        let toml_str = r#"
[[golem]]
name = "testGolem"
repo = "~/test"
"#;
        let config = Figment::from(Serialized::defaults(AppConfig::default()))
            .merge(Toml::string(toml_str))
            .extract::<AppConfig>()
            .unwrap();
        assert_eq!(config.golem.len(), 1);
        assert_eq!(config.golem[0].name, "testGolem");
    }

    #[test]
    fn golem_type_serde_roundtrip() {
        let orchestrator = GolemType::Orchestrator;
        let serialized = serde_json::to_string(&orchestrator).unwrap();
        assert_eq!(serialized, r#""orchestrator""#);
        let deserialized: GolemType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, GolemType::Orchestrator);
    }

    #[test]
    fn config_path_under_config_dir() {
        let path = config_path();
        assert!(path.to_str().unwrap().contains("golem-terminal"));
        assert!(path.to_str().unwrap().ends_with("golems.toml"));
    }

    #[test]
    fn ui_config_defaults() {
        let ui = UiConfig::default();
        assert_eq!(ui.sidebar_width, 200.0);
        assert_eq!(ui.bottom_bar_height, 24.0);
        assert_eq!(ui.pane_spacing, 2.0);
        assert_eq!(ui.font.small, 12.0);
        assert_eq!(ui.font.tiny, 10.0);
        assert_eq!(ui.font.tab, 13.0);
        assert_eq!(ui.font.group, 11.0);
        assert_eq!(ui.font.terminal, 14.0);
        assert_eq!(ui.colors.bg_primary, "#1c1c24");
        assert_eq!(ui.colors.accent, "#6699f2");
    }

    #[test]
    fn shell_config_defaults() {
        let shell = ShellConfig::default();
        assert_eq!(shell.program, "/bin/zsh");
        assert_eq!(shell.args, vec!["-l"]);
    }

    #[test]
    fn app_config_default_includes_ui_and_shell() {
        let config = AppConfig::default();
        assert_eq!(config.ui, UiConfig::default());
        assert_eq!(config.shell, ShellConfig::default());
    }

    #[test]
    fn toml_roundtrip_ui_config() {
        let toml_str = r##"
[ui]
sidebar_width = 250.0

[ui.font]
small = 14.0

[ui.colors]
accent = "#ff0000"

[shell]
program = "/bin/bash"
args = ["--norc"]
"##;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ui.sidebar_width, 250.0);
        assert_eq!(config.ui.bottom_bar_height, 24.0); // default preserved
        assert_eq!(config.ui.font.small, 14.0);
        assert_eq!(config.ui.font.tab, 13.0); // default preserved
        assert_eq!(config.ui.colors.accent, "#ff0000");
        assert_eq!(config.ui.colors.bg_primary, "#1c1c24"); // default preserved
        assert_eq!(config.shell.program, "/bin/bash");
        assert_eq!(config.shell.args, vec!["--norc"]);
    }

    #[test]
    fn toml_roundtrip_serialization() {
        let config = AppConfig::default();
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: AppConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
}

#[cfg(all(test, feature = "gui"))]
mod gui_tests {
    use super::*;

    #[test]
    fn parse_hex_color_basic() {
        let c = parse_hex_color("#ff0000");
        assert!((c.r - 1.0).abs() < 0.01);
        assert!(c.g.abs() < 0.01);
        assert!(c.b.abs() < 0.01);
        assert!((c.a - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_color_without_hash() {
        let c = parse_hex_color("00ff00");
        assert!(c.r.abs() < 0.01);
        assert!((c.g - 1.0).abs() < 0.01);
        assert!(c.b.abs() < 0.01);
    }

    #[test]
    fn parse_hex_color_invalid_returns_magenta() {
        let c = parse_hex_color("xyz");
        // Magenta fallback = (1.0, 0.0, 1.0)
        assert!((c.r - 1.0).abs() < 0.01);
        assert!(c.g.abs() < 0.01);
        assert!((c.b - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_color_with_alpha() {
        let c = super::parse_hex_color_alpha("#1f1f26", 0.7);
        assert!((c.a - 0.7).abs() < 0.01);
    }

    #[test]
    fn default_colors_parse_without_error() {
        let colors = ColorConfig::default();
        // All default hex strings should parse without hitting magenta fallback
        let all_hex = [
            &colors.bg_primary, &colors.bg_secondary, &colors.bg_sidebar,
            &colors.bg_tab_active, &colors.bg_tab_hover, &colors.accent,
            &colors.text_secondary, &colors.text_tab_active,
            &colors.status_running, &colors.status_pending, &colors.status_idle,
            &colors.focus_border,
        ];
        for hex in &all_hex {
            let c = parse_hex_color(hex);
            // Magenta fallback means broken — should NOT be magenta
            assert!(
                !((c.r - 1.0).abs() < 0.01 && c.g.abs() < 0.01 && (c.b - 1.0).abs() < 0.01),
                "Color {hex} parsed as magenta (broken)"
            );
        }
    }
}
