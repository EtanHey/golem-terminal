// Agent external state — reads /tmp/golem-agents/*.json written by Claude Code hooks.
//
// Each running Claude session writes its state to a JSON file via the
// write-agent-state.sh hook. This module reads those files periodically
// so the sidebar can display live agent status, checkpoint text, and cost.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where hook scripts write agent state files.
pub fn state_dir() -> PathBuf {
    std::env::var("GOLEM_AGENT_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/golem-agents"))
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentExternalState {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub checkpoint: String,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cost: Option<AgentCost>,
    #[serde(default)]
    pub timestamps: Option<AgentTimestamps>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentCost {
    #[serde(default)]
    pub usd_estimate: f64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentTimestamps {
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

impl AgentExternalState {
    /// Short display string for the sidebar (max ~40 chars).
    pub fn sidebar_summary(&self) -> String {
        if let Some(ref err) = self.error {
            let truncated = if err.len() > 35 {
                format!("{}...", &err[..32])
            } else {
                err.clone()
            };
            return format!("⚠ {truncated}");
        }

        match self.status.as_str() {
            "thinking" => {
                if self.checkpoint.is_empty() {
                    "Thinking...".into()
                } else {
                    truncate(&self.checkpoint, 40)
                }
            }
            "waiting_permission" => "⏸ Waiting for permission".into(),
            "waiting_input" => "⏸ Waiting for input".into(),
            "done" => {
                if let Some(ref cost) = self.cost {
                    if cost.usd_estimate > 0.0 {
                        return format!("✓ Done (${:.2})", cost.usd_estimate);
                    }
                }
                "✓ Done".into()
            }
            "error" => "⚠ Error".into(),
            "running" => {
                if self.checkpoint.is_empty() {
                    "Running...".into()
                } else {
                    truncate(&self.checkpoint, 40)
                }
            }
            _ => {
                if !self.checkpoint.is_empty() {
                    truncate(&self.checkpoint, 40)
                } else {
                    String::new()
                }
            }
        }
    }

    /// Color hint for status dot (returns a css-like color name).
    pub fn status_color_hint(&self) -> &str {
        match self.status.as_str() {
            "running" | "thinking" => "running",
            "waiting_permission" | "waiting_input" => "pending",
            "done" => "idle",
            "error" => "error",
            _ => "idle",
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agent_state_json() {
        let json = r#"{
            "status": "thinking",
            "checkpoint": "Completed Read: src/ui.rs",
            "chat_id": "abc-123",
            "cost": {"usd_estimate": 0.42, "input_tokens": 1000, "output_tokens": 500},
            "model": "claude-opus-4-6",
            "timestamps": {"started_at": "2026-03-07T00:00:00Z", "updated_at": "2026-03-07T00:01:00Z", "finished_at": null},
            "error": null
        }"#;
        let state: AgentExternalState = serde_json::from_str(json).unwrap();
        assert_eq!(state.status, "thinking");
        assert_eq!(state.checkpoint, "Completed Read: src/ui.rs");
        assert_eq!(state.chat_id.as_deref(), Some("abc-123"));
        assert_eq!(state.model.as_deref(), Some("claude-opus-4-6"));
        assert!(state.cost.is_some());
        assert!((state.cost.as_ref().unwrap().usd_estimate - 0.42).abs() < 0.001);
        assert_eq!(state.status_color_hint(), "running");
    }

    #[test]
    fn sidebar_summary_formats() {
        let running = AgentExternalState {
            status: "thinking".into(),
            checkpoint: "Completed Edit: src/main.rs".into(),
            ..Default::default()
        };
        assert_eq!(running.sidebar_summary(), "Completed Edit: src/main.rs");

        let done_with_cost = AgentExternalState {
            status: "done".into(),
            cost: Some(AgentCost {
                usd_estimate: 1.23,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(done_with_cost.sidebar_summary(), "✓ Done ($1.23)");

        let error = AgentExternalState {
            status: "error".into(),
            error: Some("Connection refused".into()),
            ..Default::default()
        };
        assert!(error.sidebar_summary().contains("Connection refused"));

        let waiting = AgentExternalState {
            status: "waiting_permission".into(),
            ..Default::default()
        };
        assert_eq!(waiting.sidebar_summary(), "⏸ Waiting for permission");
    }

    #[test]
    fn read_all_states_handles_missing_dir() {
        let states = read_all_states(std::path::Path::new("/nonexistent/path"));
        assert!(states.is_empty());
    }

    #[test]
    fn read_all_states_reads_real_dir() {
        let dir = std::env::temp_dir().join("golem-agent-state-test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("test-agent.json"),
            r#"{"status":"running","checkpoint":"Working..."}"#,
        )
        .unwrap();

        let states = read_all_states(&dir);
        assert!(states.contains_key("test-agent"));
        assert_eq!(states["test-agent"].status, "running");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Read all agent state files from the state directory.
/// Returns a map of surface name → state.
pub fn read_all_states(dir: &Path) -> HashMap<String, AgentExternalState> {
    let mut result = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return result,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }
        let surface_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if surface_name.is_empty() {
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<AgentExternalState>(&contents) {
                Ok(state) => {
                    result.insert(surface_name, state);
                }
                Err(_) => continue,
            },
            Err(_) => continue,
        }
    }

    result
}
