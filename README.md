# Golem Terminal

A native macOS terminal multiplexer built with Rust, designed for managing multiple AI agent sessions side by side. Built on [Iced](https://github.com/iced-rs/iced) 0.14 and [iced_term](https://github.com/EtanHey/iced_term) (alacritty_terminal backend).

## Features

- **Tab-based terminal management** -- multiple terminal sessions in a single window
- **Split-screen** -- primary/secondary pane layout with visual focus indicators
- **Sidebar navigation** -- Zen browser-style left sidebar with grouped, collapsible sections
- **macOS sidebar vibrancy** -- native NSVisualEffectView behind the sidebar (macOS 15+)
- **Colored accent strips** -- per-group left-border colors in the sidebar
- **TOML configuration** -- layered config via [Figment](https://github.com/SergioBenitez/Figment) (defaults -> file -> env -> CLI) with hot-reload
- **Agent state display** -- live status from external JSON state files in the sidebar
- **Programmatic control** -- Unix Domain Socket (UDS) JSON protocol for scripting and E2E testing
- **Keyboard shortcuts** -- iTerm-parity navigation (Cmd+T, Cmd+D, Cmd+B, Cmd+1-9, Cmd+Alt+Arrow)
- **PTY proxy modes** -- `wrap` and `run` subcommands for headless PTY session management

## Requirements

- Rust (2021 edition)
- macOS (sidebar vibrancy requires macOS 15+; the app runs on older versions without vibrancy)

## Building

```bash
# Build the GUI app
cargo build --release --features gui

# Install as a .app bundle to ~/Applications
./install.sh
```

## Usage

```bash
# Launch the GUI
golem-terminal ui

# Launch with a specific command instead of the default shell
golem-terminal ui -- bash

# Wrap a command in a PTY (headless, interactive)
golem-terminal wrap -- python3

# Spawn a command and wait for first output before proxying
golem-terminal run -- cargo test
```

## Configuration

Configuration lives at `~/.config/golem-terminal/golems.toml`. The file is hot-reloaded -- changes take effect without restarting.

The config supports:
- UI settings (colors, sidebar width, font size)
- Shell settings (program, args)
- Golem definitions (name, command, working directory, color, group)
- Group ordering and display

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+T | New tab |
| Cmd+D | Toggle split screen |
| Cmd+B | Toggle sidebar |
| Cmd+Alt+Arrow | Switch tabs |
| Cmd+1-9 | Select tab by number |
| Cmd+Q | Quit |

## Programmatic Control (UDS)

The app exposes a Unix Domain Socket for external control. Helper scripts are included:

- **orchestrate.py** -- Python REPL/CLI for UDS commands
- **debug.sh** -- lightweight debug REPL
- **launch.sh** -- build + launch with debug socket enabled

### Available Commands

| Command | Response | Description |
|---------|----------|-------------|
| `launch` | -- | Start terminal in slot |
| `kill` | -- | Kill terminal in slot |
| `send_input` | -- | Send keystrokes to terminal |
| `status` | `{"status":"..."}` | idle/pending/ready |
| `content` | `{"content":"hex"}` | Terminal content (hex-encoded) |
| `output` | `{"output":"hex"}` | Raw output (hex-encoded) |
| `new_tab` | -- | Create new tab |
| `close_tab` | -- | Close tab by slot |
| `select_tab` | -- | Switch active tab |
| `toggle_split` | -- | Toggle split-screen |
| `split_status` | `{"split_active":bool,...}` | Split state |
| `active_tab` | `{"active_tab":N}` | Current tab index |
| `slot_count` | `{"slot_count":N}` | Number of tabs |
| `quit` | -- | Close app |

## Architecture

```
src/
  main.rs          -- CLI entry point (wrap, run, ui subcommands)
  ui.rs            -- Iced GUI: sidebar, terminal panels, split screen, vibrancy
  config.rs        -- TOML config via Figment, hot-reload via notify
  agent_state.rs   -- Reads live agent state JSON for sidebar display
  session.rs       -- PTY lifecycle for wrap/run CLI modes
  pty.rs           -- Raw mode guard, terminal size, interactive PTY proxy
  test_harness.rs  -- UDS JSON protocol for E2E test control
```

The GUI feature is behind a Cargo feature flag (`--features gui`). Without it, only the `wrap` and `run` CLI subcommands are available.

Terminal rendering uses [iced_term](https://github.com/EtanHey/iced_term) (a fork with public backend module and async PTY drop), which wraps alacritty_terminal in an Iced Canvas widget.

## Testing

```bash
# Unit tests (71 tests, no display needed):
cargo test --features gui --bin golem-terminal

# E2E tests (5 tests, needs a display context):
cargo test --features gui --test e2e_gui
```

## License

See repository for license details.
