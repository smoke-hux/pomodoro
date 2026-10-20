# Testing

Run `npm test` for React/domain helper tests and `cargo test --locked --manifest-path src-tauri/Cargo.toml` for backend tests. `npm run build` checks TypeScript and produces the frontend bundle. CI also runs lint, formatting and dependency checks. The backend and desktop smoke jobs run independently on Ubuntu 24.04 and 26.04, with separate native dependency caches and screenshot artifacts.

Tagged releases build in an Ubuntu 22.04 container on an Ubuntu 26.04 runner to preserve the older glibc baseline. Before attaching the package to a release, installation jobs on Ubuntu 24.04 and 26.04 resolve the generated Debian dependencies and check the installed binary for missing shared libraries. Those jobs verify packaging and linkage; the desktop smoke and manual GNOME checks cover behavior.

## Native desktop smoke test

The Linux smoke test launches the compiled application through Tauri's native WebDriver bridge. It exercises real UI controls and the Rust backend: creating/selecting a task with the keyboard, saving settings, starting/pausing/resuming, preserving paused state and a running deadline across process restarts, completing a real one-minute interval exactly once, and recovering a corrupted store. It takes about 90 seconds after compilation.

Review the native test prerequisites with `bash scripts/setup-linux.sh --print --desktop`, then install them with `bash scripts/setup-linux.sh --install --desktop`. Ubuntu 26.04 uses `webkitgtk-webdriver`; Ubuntu 22.04 and 24.04 use `webkit2gtk-driver`. Both provide `WebKitWebDriver`. The script also includes Xvfb, xauth and the D-Bus tools used by the isolated harness. Install the same bridge version as CI, then run:

```sh
cargo install tauri-driver --locked --version 2.0.6
npm run tauri -- build --debug --no-bundle
GDK_BACKEND=x11 xvfb-run -a npm run test:desktop
```

Use the Tauri build command, not plain `cargo build`: the test needs the embedded frontend instead of a connection to Vite. `POMODORO_TEST_BINARY` can name another compiled binary; `TAURI_DRIVER` and `WEBKIT_WEBDRIVER` can name explicit driver paths. The harness deliberately has no extra npm dependencies; it uses Node's built-in WebDriver HTTP client code.

Set `POMODORO_TEST_ARTIFACTS` to an output directory to retain WebView screenshots of paused focus, completed focus, recovery and failures. Only the generated smoke fixture appears in these images.

Every run creates a private temporary directory with separate XDG data, configuration, cache and runtime directories, starts a private D-Bus session, and uses the in-memory settings backend. The fixture disables sound, desktop alerts, notification capture and desktop banner control. It does not read the user's Pomodoro data or change desktop settings. The test terminates only its own process group, including processes left in the tray. Successful runs remove their temporary fixture; failures print its location and retain it for diagnosis. A missing driver is an explicit failure, not a skipped passing test.

The native driver setup follows [Tauri's manual WebDriver guide](https://v2.tauri.app/develop/tests/webdriver/manual-setup/) and its [Linux CI guidance](https://v2.tauri.app/develop/tests/webdriver/ci/).

## Checks requiring a real desktop

The headless smoke test uses Xvfb even on Ubuntu 26.04; it cannot assess native Wayland, a GNOME tray extension or a screen reader. Before a release, check these behaviors in a disposable GNOME session on Ubuntu 26.04 (Wayland), with regression checks on 24.04 and 22.04:

- Opening, resizing, fractional display scaling, moving between displays and restoring the window.
- Closing and restoring through the tray when an AppIndicator host is enabled; safe window behavior when no tray host is present; explicit quit.
- Timer deadlines across suspend/resume, a running timer restart, and a paused timer restart.
- Notification capture opt-in, unread messages and Do Not Disturb restoration without changing another application's settings.
- Sound playback with PipeWire, and useful feedback when a player, sound theme or desktop integration is unavailable.
- Keyboard-only navigation, Orca announcements, light/dark themes, high contrast and reduced motion.

Do not set `GDK_BACKEND=x11` for the Wayland checks. Native notification/desktop integration tests remain opt-in because they require that environment. Record which desktop checks were actually performed; passing the headless job alone is not evidence that a GNOME session was tested.

## Frontend organization

`App.tsx` composes the page and modal/theme state. `useAppSnapshot` owns subscription/read ordering and connection cleanup; `useAppCommands` owns stable command callbacks and feedback; `useKeyboardShortcuts` owns global keyboard arbitration; `useNotice` owns announcement lifetime. Browser-only sample data lives in `browserPreview.ts`.

`src/styles.css` is the ordered stylesheet entry point. Component styles live in `src/styles/`; preserve the import order when editing because responsive, accessibility and state rules intentionally follow the foundations they override.
