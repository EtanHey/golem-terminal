# Golem Terminal

Tab-based terminal multiplexer for Claude Code orchestration. Built with Rust + Iced 0.14 + iced_term (alacritty_terminal backend).

## Architecture

- **src/main.rs** — CLI entry (wrap, run, ui subcommands)
- **src/ui.rs** — Iced GUI: sidebar navigation, iced_term terminal panels, split screen, focus model
- **src/session.rs** — PTY lifecycle for wrap/run CLI modes (not used by GUI — iced_term manages its own PTY)
- **src/pty.rs** — Raw mode guard, terminal size, interactive PTY proxy
- **src/test_harness.rs** — UDS JSON protocol for E2E test control

## V2 Changes (iced_term)

- **Terminal rendering:** iced_term (Canvas widget + alacritty_terminal) replaces vt100 + text() widget
- **Navigation:** Left sidebar (Zen browser style) replaces top tab bar
- **No toolbar:** Removed Filtered/Raw toggle — iced_term renders terminal natively
- **No auto-launch:** SelectTab only switches view, LaunchSlot is explicit
- **Focus model:** PaneSide (Primary/Secondary) with visual focus indicators
- **macOS:** Transparent titlebar + fullsize content view
- **Keyboard input:** Handled by iced_term natively, GUI shortcuts (Cmd+T/D/B/Q) intercepted separately

## Rust Conventions

- RAII guards for stateful resources (`RawModeGuard`)
- `\r` (CR) for Enter in PTY input, not `\n`
- Use `anyhow` for fallible returns
- No magic numbers — all literals are named constants

## Programmatic Control

- **orchestrate.py** — Python script for UDS control (REPL, demo, CLI)
- **debug.sh** — Lightweight debug REPL for connected terminal
- **launch.sh** — Build + launch with debug socket enabled
- **install.sh** — Build + install .app to ~/Applications for dock launcher

### UDS Commands

| Command | Response | Description |
|---------|----------|-------------|
| `launch` | (none) | Start iced_term terminal in slot |
| `kill` | (none) | Kill terminal in slot |
| `send_input` | (none) | Send keystrokes to terminal in slot |
| `status` | `{"status":"..."}` | idle/pending/ready |
| `content` | `{"content":"hex"}` | Terminal content (hex) |
| `output` | `{"output":"hex"}` | Raw output (hex) |
| `new_tab` | (none) | Create new tab |
| `close_tab` | (none) | Close tab by slot |
| `select_tab` | (none) | Switch active tab |
| `toggle_split` | (none) | Toggle split-screen |
| `split_status` | `{"split_active":bool,...}` | Split state |
| `active_tab` | `{"active_tab":N}` | Current tab index |
| `slot_count` | `{"slot_count":N}` | Number of tabs |
| `quit` | (none) | Close app |

## Testing

```bash
# Unit tests (25 tests, no display needed):
cargo test --features gui --bin golem-terminal

# E2E tests (needs display):
cargo test --features gui --test e2e_gui
```

## Key Shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+T | New tab |
| Cmd+D | Toggle split screen |
| Cmd+B | Toggle sidebar |
| Cmd+Alt+Arrow | Switch tabs |
| Cmd+1-9 | Select tab by number |
| Cmd+Q | Quit |

## Known Limitations

- ~~`send_input` UDS command is no-op~~ **FIXED** — forked iced_term (EtanHey/iced_term), made backend module pub. send_input now works.
- ~~Pty::drop() blocks synchronously~~ **FIXED** — fork moves PTY shutdown to background thread, UI stays responsive.
- Vibrancy/Liquid Glass deferred — requires objc FFI for sidebar-only NSVisualEffectView
- Tab groups not yet implemented — all tabs in single "AGENTS" group
