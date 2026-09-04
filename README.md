# Pomodoro

Pomodoro is a local-first focus timer for Ubuntu. It follows the Pomodoro Technique: focused work intervals, short breaks, and a long break after four completed rounds.

## What it includes

- A 25-minute focus, 5-minute short-break, and 15-minute long-break cycle by default
- Configurable durations, round count, automatic transitions, notifications, sound, and theme
- A task list with focus-session estimates and completed-session counts
- A warning when a task is estimated above four sessions, encouraging smaller steps
- Fast interruption capture that does not stop the timer
- Optional desktop notification capture, so what other apps send during a focus interval is read afterwards
- A daily session ledger with planned capacity, completed focus time, and interruptions
- Accurate absolute deadlines across minimized windows, screen locks, and laptop suspend
- Local JSON persistence, system-tray controls, and single-instance behavior
- Keyboard navigation, high-contrast support, reduced-motion support, and light/dark themes

Pomodoro is a standalone app. It does not require an account, use cloud sync, or send task data anywhere.

## Install on Ubuntu

Install the generated Debian package:

    sudo apt install ./src-tauri/target/release/bundle/deb/Pomodoro_0.1.0_amd64.deb

Open Pomodoro from the Ubuntu application launcher. Closing the window hides it in the system tray so an active timer can continue. Choose Quit from the tray menu to exit completely.

This local package was built on Ubuntu 24.04 x86_64 and is directly verified for Ubuntu 24.04 or newer. To support Ubuntu 22.04 with the widest binary compatibility, build the release on Ubuntu 22.04.

## Use

1. Add a task and estimate how many focus sessions it needs.
2. Select the task, then start the focus timer.
3. Use Ctrl+I to note a distraction without leaving the session.
4. When focus ends, take the prompted short break.
5. After four completed focus sessions, take the longer restorative break.
6. Review the Today ledger to compare the plan with completed work.

The Space key starts, pauses, and resumes the active timer. Ctrl+N adds a task. Ctrl+1, Ctrl+2, and Ctrl+3 choose focus, short break, or long break while idle. Ctrl+, opens settings.

## Desktop notification capture

Pomodoro can watch the desktop notification service and file a copy of what other applications send, so it can be read after a focus interval instead of during one. It is **off by default** and does nothing until you turn it on in Settings → Notification capture.

What it does and does not do:

- It **files a copy**. Filing is passive: Pomodoro observes the notification after the desktop has already accepted it.
- It **does not hide the banner**. Watching a notification cannot stop it. Banners still appear and sounds still play.
- **Silence banners during focus** is the setting that actually quiets the desktop. It switches GNOME's own Do Not Disturb on for the length of each focus interval and switches it back at the end. It is a separate, off-by-default toggle because it changes a setting that belongs to the desktop rather than to Pomodoro. Banners you had already turned off yourself are left alone, and if Pomodoro is killed mid-interval the next launch puts the setting back.

You choose what is worth keeping: a minimum urgency, a list of muted apps, a list of priority apps that bypass the other rules, and whether to capture outside focus intervals at all. Pomodoro never captures its own notifications.

Captured summaries and bodies routinely contain message text and one-time codes. They are written only to the local data file below, which Pomodoro keeps owner-only (`0600`, in a `0700` directory). They are never logged and never leave the machine — the app makes no network requests. Settings has an explicit, confirmed action for deleting every captured copy.

Capture needs a session bus that will hand out a monitor connection, which is the case on a standard Ubuntu GNOME desktop. If it cannot start, Settings and the inbox say so rather than showing an empty list as though nothing had arrived.

## Local data

Application data is stored beneath the standard Linux user data directory, normally:

    ~/.local/share/app.pomodoro.timer/pomodoro.json

The file holds captured notification text, so Pomodoro restricts it to its owner (`0600`) inside an owner-only directory (`0700`) on every save. A store written by an earlier build is tightened the next time the app saves.

Settings includes explicit, confirmed actions for clearing session history and for deleting every captured notification. Removing the application does not automatically remove this local data file.

### Upgrading from the Kipindi build

This app was previously named Kipindi and used the bundle identifier `app.kipindi.timer`, which put its data at a different path. If you ran that build, carry your tasks, settings, and session history across once:

    mkdir -p ~/.local/share/app.pomodoro.timer
    cp ~/.local/share/app.kipindi.timer/kipindi.json \
       ~/.local/share/app.pomodoro.timer/pomodoro.json

The old directory is left in place. Remove it once the new build has started cleanly and your history looks right.

## Develop

Required Ubuntu packages:

    sudo apt install libwebkit2gtk-4.1-dev build-essential file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

Install project dependencies and run the checks:

    npm install
    npm test
    npm run build
    cargo test --manifest-path src-tauri/Cargo.toml

Run the desktop app in development:

    npm run tauri dev

Build a Debian package:

    npm run tauri build -- --bundles deb
