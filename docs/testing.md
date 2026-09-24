# Testing

Run `npm test` for React/domain helper tests and `cargo test --manifest-path src-tauri/Cargo.toml` for backend tests. `npm run build` checks TypeScript and produces the frontend bundle. CI also runs lint, formatting and dependency checks.

## Native desktop smoke test

The Linux smoke test launches the compiled application through Tauri's native WebDriver bridge. It exercises real UI controls and the Rust backend: creating/selecting a task with the keyboard, saving settings, starting/pausing/resuming, preserving paused state and a running deadline across process restarts, completing a real one-minute interval exactly once, and recovering a corrupted store. It takes about 90 seconds after compilation.

Install the normal Tauri build prerequisites, plus `webkit2gtk-driver`, `xvfb`, and `dbus-daemon` on Ubuntu. Install the bridge with `cargo install tauri-driver --locked`, then run:

```sh
npm run tauri -- build --debug --no-bundle
xvfb-run -a npm run test:desktop
```

Use the Tauri build command, not plain `cargo build`: the test needs the embedded frontend instead of a connection to Vite. `POMODORO_TEST_BINARY` can name another compiled binary; `TAURI_DRIVER` and `WEBKIT_WEBDRIVER` can name explicit driver paths. The harness deliberately has no extra npm dependencies; it uses Node's built-in WebDriver HTTP client code.

Set `POMODORO_TEST_ARTIFACTS` to an output directory to retain WebView screenshots of paused focus, completed focus, recovery and failures. Only the generated smoke fixture appears in these images.

Every run creates a private temporary directory with separate XDG data, configuration, cache and runtime directories, starts a private D-Bus session, and uses the in-memory settings backend. The fixture disables sound, desktop alerts, notification capture and desktop banner control. It does not read the user's Pomodoro data or change desktop settings. The test terminates only its own process group, including processes left in the tray. Successful runs remove their temporary fixture; failures print its location and retain it for diagnosis. A missing driver is an explicit failure, not a skipped passing test.

The native driver setup follows [Tauri's manual WebDriver guide](https://v2.tauri.app/develop/tests/webdriver/manual-setup/) and its [Linux CI guidance](https://v2.tauri.app/develop/tests/webdriver/ci/).

## Checks requiring a real desktop

The smoke test cannot assess a GNOME tray extension or a screen reader through WebDriver. Before a release, check closing to tray and restoring, explicit quit, notification capture opt-in, Do Not Disturb restoration, suspend/resume, keyboard-only navigation, Orca announcements, light/dark themes, high contrast and reduced motion in a supported Ubuntu/GNOME session. Native notification/desktop integration tests remain opt-in because they require that environment.

## Frontend organization

`App.tsx` composes the page and modal/theme state. `useAppSnapshot` owns subscription/read ordering and connection cleanup; `useAppCommands` owns stable command callbacks and feedback; `useKeyboardShortcuts` owns global keyboard arbitration; `useNotice` owns announcement lifetime. Browser-only sample data lives in `browserPreview.ts`.

`src/styles.css` is the ordered stylesheet entry point. Component styles live in `src/styles/`; preserve the import order when editing because responsive, accessibility and state rules intentionally follow the foundations they override.
