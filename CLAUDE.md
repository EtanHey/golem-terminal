# Golem Terminal

Tab-based terminal multiplexer for Claude Code orchestration. Built with Rust + Iced 0.14.

## Architecture

- **src/main.rs** — CLI entry (wrap, run, ui subcommands)
- **src/ui.rs** — Iced GUI: tab bar, terminal panels, split screen, keyboard routing
- **src/session.rs** — PTY lifecycle (spawn, kill, send, output streaming)
- **src/pty.rs** — Raw mode guard, terminal size, interactive PTY proxy
- **src/test_harness.rs** — UDS JSON protocol for E2E test control

## Rust Conventions

- RAII guards for stateful resources (`RawModeGuard`)
- `\r` (CR) for Enter in PTY input, not `\n`
- Use `anyhow` for fallible returns
- No magic numbers — all literals are named constants

## PTY Input Rule

Trailing control chars (bytes < 0x20, except tab) MUST be split from text and
deferred via `Task::done(Message::SendInput(...))`. Raw-mode programs only
recognize control chars when they arrive as a separate `read()` event.

## Testing

```bash
# Unit tests (16 tests, no display needed):
cargo test --features gui --bin golem-terminal

# E2E tests (needs display):
cargo test --features gui --test e2e_gui
```

## Key Shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+T | New tab |
| Cmd+D | Toggle split screen |
| Cmd+Alt+Arrow | Switch tabs |
| Cmd+1-9 | Select tab by number |
| Cmd+V | Paste |
| Cmd+Q | Quit |
