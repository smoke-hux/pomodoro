# Contributing

Pomodoro is an Ubuntu desktop application with a React/TypeScript interface and a Tauri/Rust backend. Keep changes focused and explain the user-visible behavior in the pull request.

## Development setup

Use Node.js 22.14 or newer and stable Rust. Ubuntu 24.04 is the CI test environment; release packages are currently built on Ubuntu 22.04 x86_64 to retain compatibility with that release. GNOME 42 (Ubuntu 22.04) and GNOME 46 (Ubuntu 24.04) are the reference desktop environments. Other Linux desktops may run the timer, but GNOME Do Not Disturb and notification monitoring need explicit desktop verification. Windows and macOS are not supported release targets.

Install the native packages listed in [README.md](README.md#develop), then run `npm ci`. Start the desktop application with `npm run tauri dev`. The browser preview (`npm run dev`) uses sample data and cannot verify native behavior.

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
