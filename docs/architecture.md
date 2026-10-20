# Architecture

The Rust backend owns application data, absolute timer deadlines and persistence. React renders snapshots and calculates the visible countdown locally. A command applies a change; a `state-changed` event publishes its result. The initial snapshot is read after event subscription so a change cannot disappear between initialization steps.

## Backend boundaries

| Module                 | Responsibility                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------ |
| `lib.rs`               | Compose plugins, state, window lifecycle and command registration                                |
| `runtime.rs`           | Reconcile deadlines, persist state, schedule work, coordinate capture and desktop silence        |
| `commands.rs`          | Validate and apply ordinary user commands                                                        |
| `domain/`              | Timer transitions, tasks/interruptions, notification capture and settings; independent of the UI |
| `tray.rs`, `alerts.rs` | Tray controls and desktop notification text                                                      |
| `sound/`               | Sound-theme lookup and playback scheduling                                                       |
| `storage.rs`           | Private atomic writes, unreadable-file recovery and rotating backups                             |
| `schema.rs`            | Disk schema versions, legacy migration and import validation                                     |
| `data_transfer.rs`     | Native file dialogs, import preview/commit, complete JSON and CSV exports                        |

Tests live beside their domains. Runtime and storage tests use temporary directories; live GNOME checks remain explicitly ignored in normal test runs.

## Frontend boundaries

`App.tsx` composes the main views and coordinates theme and modal state. The extracted hooks own snapshot synchronization, command callbacks, keyboard shortcuts and notice lifetime. Sample data is isolated in `browserPreview.ts`. `DataSettings` handles transfer status and confirmation; only counts and an opaque confirmation token cross into the import preview. The selected file is validated and retained by Rust, so confirmation imports exactly the data that was previewed.

`src/styles.css` imports component styles in their original cascade order. The existing palette and typography are preserved. New transfer controls reuse the existing form and button patterns.

## Data changes and recovery

Store schema and application version are separate. Version 0 is the original unversioned JSON object; version 1 adds an explicit `schemaVersion` while retaining the field layout. Add future sequential migrations in `schema.rs`, with fixtures that prove old data survives.

Import validation finishes before the current state is locked for replacement. Confirmation requires an idle timer and a writable store. Under the state lock, the old state is backed up, the new state is written atomically, and only then is the in-memory state replaced and broadcast. A failed backup or replacement leaves the current state intact. A local desktop restore marker is retained; imported settings never enable capture or desktop silence automatically.

Backups preserve the complete old state, including private captured text. Deleting data from the active store does not retroactively scrub prior backups or manual exports. The UI and README explain this explicitly.

## Verification and remaining desktop checks

`npm run check`, `cargo fmt --all --check`, strict Clippy and Cargo tests are the local quality gate. `npm run test:desktop` drives the real Linux app using WebDriver with isolated data and desktop settings; [Testing](testing.md) documents the setup and coverage.

Actual tray restoration, Orca announcements, GNOME notification monitoring, suspend/resume and desktop-silence behavior still need a supported desktop session. Unit and WebDriver tests do not prove those environment-dependent integrations.
