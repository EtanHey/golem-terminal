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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            golem: vec![],
            groups: HashMap::new(),
        }
    }
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

# Example:
# [[golem]]
# name = "orcClaude"
# repo = "~/Gits/orchestrator"
# icon = "🎭"
# color = "#7C3AED"
# type = "orchestrator"
# command = ["claude", "code"]

# [groups]
# orchestrators = ["orcClaude"]
# workers = ["brainClaude", "golemsClaude"]
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
}
