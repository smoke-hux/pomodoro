# Security policy

Security fixes target the latest released version and the current `main` branch. This is a community project; no response-time or long-term support commitment is implied.

## Reporting a vulnerability

Use GitHub's [private vulnerability reporting form](https://github.com/smoke-hux/pomodoro/security/advisories/new) if it is available. Include the affected version, platform, impact, and a minimal reproduction using fabricated data. The repository owner must enable private vulnerability reporting in GitHub settings for that form to accept reports.

If the private form is unavailable, open a public issue asking the maintainer for a private reporting channel without posting vulnerability details or sensitive data. Never attach a live `pomodoro.json`, exported backup, captured notification body, credential, or one-time code to an issue.

## Data and trust boundaries

Pomodoro stores data locally. Notification capture is opt-in, and captured messages can be sensitive even after the notification disappears from the desktop. Exports and manually copied backups deserve the same care as the live data file. Files supplied for import must be treated as untrusted data.

The application does not require a cloud account or runtime network access. Its build tools, dependency downloads, and release infrastructure do use the network. Automated dependency and CodeQL workflows help identify changes requiring review; they do not guarantee that every issue is detected.

## Dependency review — 2026-09-24

The npm advisory scan reports zero vulnerabilities after upgrading Vitest to 4.1.11. Cargo's advisory scan completed against RustSec database commit `593df8c1b5ed0bcde9dddadfeeead776fa514ff8` and reported seven upstream warnings in the locked dependency graph:

- `glib` 0.18.5: [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), an unsoundness advisory fixed in GLib bindings 0.20.0 and later. The current Tauri/GTK dependency tree still includes the 0.18 series; upgrading that shared binding needs a compatible upstream dependency change.
- `proc-macro-error` 1.0.4 and five `unic-*` 0.9.0 packages: unmaintained dependency warnings.

These findings remain visible; no advisory IDs are suppressed. `cargo audit` permits these informational warning categories by default, so a successful exit does not mean there are no upstream concerns. Review the warnings on dependency updates and follow the upstream replacements. The local verification used `--no-yanked` because registry-cache access was restricted; CI runs the default audit, including registry checks.
