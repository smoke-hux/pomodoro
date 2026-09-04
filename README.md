# Pomodoro

Pomodoro is a local-first focus timer for Ubuntu. It follows the Pomodoro Technique: focused work intervals, short breaks, and a long break after four completed rounds.

## What it includes

- A 25-minute focus, 5-minute short-break, and 15-minute long-break cycle by default
- Configurable durations, round count, automatic transitions, notifications, sound, and theme
- A task list with focus-session estimates and completed-session counts
- A warning when a task is estimated above four sessions, encouraging smaller steps
- Fast interruption capture that does not stop the timer
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

## Local data

Application data is stored beneath the standard Linux user data directory, normally:

    ~/.local/share/app.pomodoro.timer/pomodoro.json

Settings includes an explicit, confirmed action for clearing session history. Removing the application does not automatically remove this local data file.

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

## Method reference

The behavior is based on Todoist's overview of the Pomodoro Technique: https://www.todoist.com/productivity-methods/pomodoro-technique

Pomodoro is an original, independent application. It is not affiliated with Todoist, nor with Francesco Cirillo, who created the Pomodoro Technique and holds the associated trademarks.
