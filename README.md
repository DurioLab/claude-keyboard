# Claude Virtual Keyboard

A Tauri-based macOS app that provides a virtual keyboard for Claude Code permission confirmations.

## How It Works

1. App installs hooks into `~/.claude/hooks/` and registers them in `~/.claude/settings.json`
2. When Claude Code needs permission to run a tool, the hook script sends the event via Unix socket
3. The app pops up a 3-button virtual keyboard: **Reject** | **Allow Always** | **Allow Once**
4. Your decision is sent back to Claude Code through the hook

## Features

- 🎹 Three-button virtual keyboard UI styled like physical key caps
- ⌨️ Arrow keys (← →) to navigate, Enter to confirm, Esc to reject
- 🔒 "Allow Always" adds the tool to a session whitelist (auto-approves future requests)
- 🔌 Unix socket communication with Claude Code hooks
- 🪟 Transparent, always-on-top, borderless window

## Build

```bash
# Prerequisites: Rust 1.70+, Node.js 18+
cargo build                  # Debug build
pnpm run build              # Production build (creates .app bundle)
```

## Run

```bash
# Run in dev mode
cd src-tauri && RUST_LOG=info cargo run

# Or open the built .app
open src-tauri/target/debug/bundle/macos/Claude\ Keyboard.app
```

## Test

```bash
# While the app is running, simulate a permission request
python3 test_permission.py Bash "rm -rf /tmp/test"
```

## Architecture

```
Claude Code → Hook Script (Python) → Unix Socket → Tauri App (Rust + HTML)
                                                         ↓
                                                    User clicks button
                                                         ↓
Claude Code ← Hook Script ← Unix Socket ← Decision (allow/deny)
```

## Uninstall Hooks

The hooks are automatically installed on app launch. To remove them, delete the relevant entries from `~/.claude/settings.json` and remove `~/.claude/hooks/claude-keyboard.py`.
