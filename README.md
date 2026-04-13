<div align="center">

# Claude Keyboard

**A Dynamic Island-style permission controller for Claude Code.**

[![GitHub stars](https://img.shields.io/github/stars/DurioLab/claude-keyboard?style=flat-square)](https://github.com/DurioLab/claude-keyboard/stargazers)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey?style=flat-square)](#)
[![Version](https://img.shields.io/badge/version-0.1.7-green?style=flat-square)](#)

Approve, reject, or whitelist Claude Code permissions — with a click, a keystroke, or your voice.

<!-- TODO: Replace with actual demo GIF -->
![Demo](https://via.placeholder.com/800x450.png?text=Demo+GIF+Coming+Soon)

[Website](https://duriolab.github.io/claude-keyboard) · [Download](https://github.com/DurioLab/claude-keyboard/releases) · [Report Bug](https://github.com/DurioLab/claude-keyboard/issues)

</div>

---

## Features

🏝️ **Dynamic Island UI** — Sits as a minimal pill when idle, smoothly expands into a three-button keyboard when a permission request arrives.

🎹 **Three Clear Actions** — **Once** (allow this time) · **Always** (whitelist for the session) · **Reject** (deny and move on).

⌨️ **Keyboard-First** — `←` `→` to navigate, `Enter` to confirm, `Esc` to reject. Never leave the keyboard.

🎙️ **Voice Control** — Say "allow" or "reject" and it just works. Powered by local Whisper inference — nothing leaves your machine.

🍄 **Mario Sound Effects** — Satisfying audio feedback on every action. Because why not.

🪟 **Stays Out of Your Way** — Transparent, borderless, always-on-top. Feels native, not bolted on.

🖥️ **Cross-Platform** — macOS (Apple Silicon) and Windows. Unix Socket on Mac, Named Pipe on Windows.

---

## Quick Start

### Option 1: One-Line Install (macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/DurioLab/claude-keyboard/main/install.sh | bash
```

Auto-detects Apple Silicon / Intel, downloads the latest release, installs to `/Applications`, and removes quarantine. Done.

### Option 2: Download Manually

Grab the latest release for your platform:

→ [**GitHub Releases**](https://github.com/DurioLab/claude-keyboard/releases)

Launch the app. It auto-installs the Claude Code hooks on startup.

### Option 3: Build from Source

```bash
# Prerequisites: Rust 1.70+, Node.js 18+, pnpm
git clone https://github.com/DurioLab/claude-keyboard.git
cd claude-keyboard

# Dev mode
cd src-tauri && RUST_LOG=info cargo run

# Production build
pnpm run build
```

---

## How It Works

```
┌──────────────┐     ┌──────────────┐     ┌─────────────────────┐
│  Claude Code │────▶│  Hook Script │────▶│  IPC                │
│              │     │  (Python)    │     │  Unix Socket (mac)  │
│              │     │              │     │  Named Pipe (win)   │
└──────┬───────┘     └──────────────┘     └──────────┬──────────┘
       │                                             │
       │                                             ▼
       │                                  ┌─────────────────────┐
       │                                  │  Claude Keyboard    │
       │                                  │  (Tauri + Rust)     │
       │                                  │                     │
       │                                  │  [Once] [Always]    │
       │                                  │       [Reject]      │
       │                                  └──────────┬──────────┘
       │                                             │
       ▼                                             ▼
┌──────────────┐                          ┌─────────────────────┐
│  Continues / │◀─────────────────────────│  User Decision      │
│  Stops       │      IPC response        │  click / key / voice│
└──────────────┘                          └─────────────────────┘
```

1. Claude Code triggers a permission hook.
2. The hook script sends the request over IPC.
3. Claude Keyboard expands from pill → keyboard.
4. You decide. The response flows back instantly.

---

## Voice Control

Claude Keyboard includes built-in voice recognition powered by [whisper-rs](https://github.com/tazz4843/whisper-rs) — fully local, no network calls.

| Command | Action |
|---------|--------|
| "allow" / "yes" / "once" | Allow once |
| "always" | Allow always |
| "reject" / "no" / "deny" | Reject |

Voice recognition activates automatically when a permission request is pending.

---

## Tech Stack

| Layer | Tech |
|-------|------|
| App framework | [Tauri 2](https://tauri.app/) |
| Backend | Rust |
| Frontend | HTML / CSS / JS |
| Voice | whisper-rs (local) |
| IPC | Unix Socket / Named Pipe |

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## License

[MIT](LICENSE)

---

<div align="center">

Built with ❤️ by [DurioLab](https://github.com/DurioLab)

</div>
