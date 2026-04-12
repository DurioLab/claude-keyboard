# Contributing

Thanks for your interest in contributing! Whether it's a bug fix, new feature, or documentation improvement — all contributions are welcome.

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) 1.70+
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/)

**macOS:** Install Xcode Command Line Tools:

```bash
xcode-select --install
```

**Windows:** Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **C++ workload**.

### Getting Started

```bash
# Clone your fork
git clone https://github.com/<your-username>/claude-virtual-keyboard.git
cd claude-virtual-keyboard

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev
```

## Project Structure

```
src/          → Frontend (HTML/CSS/JS)
src-tauri/    → Rust backend (Tauri 2)
docs/         → Landing page (GitHub Pages)
```

## How to Contribute

1. **Fork** the repo and clone it locally
2. **Create a branch** for your change: `git checkout -b feat/my-feature`
3. **Make your changes** and commit
4. **Push** to your fork and open a **Pull Request**

### Commit Messages

Please use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add keyboard shortcut for copy
fix: resolve layout shift on resize
docs: update README with build instructions
```

### Pull Requests

- Describe **what** you changed and **why**
- Link related issues if applicable
- Keep PRs focused — one concern per PR

## Bug Reports

Found a bug? [Open an issue](../../issues/new) and include:

- Your OS and version
- App version
- Steps to reproduce
- Expected vs actual behavior

## Feature Requests

Have an idea? [Open an issue](../../issues/new) describing:

- The use case
- How you'd expect it to work

## Code Style

- **Rust:** Run `cargo fmt` and `cargo clippy` before committing
- **Frontend:** Follow the existing code style

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
