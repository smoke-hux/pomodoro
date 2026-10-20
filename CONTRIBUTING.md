# Contributing

Pomodoro is an Ubuntu desktop application with a React/TypeScript interface and a Tauri/Rust backend. Keep changes focused and explain the user-visible behavior in the pull request.

## Development setup

Use Node.js 22.14 or newer and stable Rust. Ubuntu 26.04 LTS is the current desktop target; backend and native smoke CI run on both Ubuntu 24.04 and 26.04. Release packages retain Ubuntu 22.04 x86_64 as their build baseline inside a container on a supported runner, then must install successfully on both newer LTS runners before publication. GitHub's deprecated Ubuntu 22.04 runner is not required.

The reference desktops are GNOME 42 on Ubuntu 22.04, GNOME 46 on 24.04, and GNOME 50 on 26.04. Ubuntu 26.04's GNOME session uses Wayland. Keep GTK/WebKit's backend selection automatic in the application; headless CI uses Xvfb explicitly and does not establish native Wayland compatibility. Other Linux desktops need explicit verification of desktop integration. Windows and macOS are not supported release targets.

Install Node.js and Rust separately, then review and install the shared native prerequisites:

```sh
bash scripts/setup-linux.sh --print
bash scripts/setup-linux.sh --install
npm ci
npm run tauri dev
```

The setup script supports Ubuntu 22.04, 24.04 and 26.04, defaults to printing the package command, and only invokes apt with `--install`. It does not install Node.js or Rust, enable repositories, or change desktop settings. Ubuntu's Universe repository and normal updates/security repositories must be enabled for the native packages. Add `--desktop` when setting up the WebDriver smoke test. On Ubuntu 26.04 this selects `webkitgtk-webdriver`; on older LTS releases it selects `webkit2gtk-driver`.

The browser preview (`npm run dev`) uses sample data and cannot verify native behavior. See [the testing guide](docs/testing.md) for isolated desktop tests and the manual release checks. The prerequisites follow [Tauri's Linux requirements](https://v2.tauri.app/start/prerequisites/); [Ubuntu's WebDriver manual](https://manpages.ubuntu.com/manpages/resolute/man1/WebKitWebDriver.1.html) documents the current driver package.

## Before opening a pull request

Run from the repository root:

```sh
npm run format:check
npm run lint
npm run build
npm test
npm run licenses:check
cargo fmt --manifest-path src-tauri/Cargo.toml --all --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

`npm run format` applies Prettier to frontend code, documentation, and configuration; Rust uses `cargo fmt`. ESLint checks TypeScript, React hook dependencies, and JSX accessibility. Prefer fixing the cause of a finding. If a particular rule cannot model correct behavior, explain a narrowly scoped suppression beside the relevant code.

Add or adjust tests when behavior changes. Use temporary directories for storage and migration tests. Never use real notification content or the developer's live application store in fixtures. Unit tests must not send notifications, play sounds, or change desktop settings. Live desktop integration checks are explicitly ignored in the normal Rust suite; see the README before running them.

## Dependencies and security checks

Commit both npm and Cargo lockfiles when dependencies change. Dependabot checks npm, Cargo, and GitHub Actions weekly. The dependency workflow runs on lockfile changes, weekly, and manually; it runs `npm audit --audit-level=high`, `cargo audit`, and license checks. Moderate npm advisories are still printed for review. Do not suppress an advisory without documenting its relevance, mitigation, and review date.

`npm run licenses:check` checks the SPDX metadata of every package in the npm lockfile, including build tools and optional platform packages. `cargo deny --locked check licenses`, run from `src-tauri` after installing `cargo-deny`, checks Rust dependencies. A new or missing license fails for review; retain all required upstream notices when distributing software. These checks are an inventory policy, not legal advice.

CodeQL scans JavaScript/TypeScript, Rust, and workflow files. The repository must have GitHub code scanning enabled for results to be published. Local linting and tests remain required even when a hosted scan cannot run.

## Review expectations

Preserve local-first behavior, explicit consent for notification capture, keyboard access, and reduced-motion/high-contrast support. Keep backend timer deadlines authoritative. Explain any storage schema changes and provide migration coverage and recovery behavior. Include screenshots for visible changes and note any remaining manual screen-reader or GNOME checks.

Use [SECURITY.md](SECURITY.md) for security reports and the issue template for ordinary bugs. Do not upload `pomodoro.json`: it may contain private messages and one-time codes.
