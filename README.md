# Pomodoro

A local-first focus timer for Ubuntu, built on the Pomodoro Technique: work in focused intervals, take a short break after each one, and a longer break after four. It lives in the system tray, keeps a plan and a record of the day, catches the things that interrupt you so you can deal with them afterwards, and tells you — with your desktop's own sound and a notification — when an interval is over.

It is a standalone desktop app. There is no account, no cloud, no sync and no network access at all: everything it knows is in one file in your home directory.

## What it does

**The timer.** A 25-minute focus, a 5-minute short break and a 15-minute long break after four rounds, all adjustable. Breaks can start by themselves when a focus ends, and the next focus can start by itself when a break ends. The deadline is an absolute moment, not a count of ticks, so an interval ends on time whether the window was minimised, the screen was locked or the laptop was asleep. Seven ways of showing the time left — plain digits, a ring, pips, a bar, words, an analogue face, a vessel that empties — are under Settings → Appearance.

**Tasks.** Each task carries an estimate of how many focus sessions it needs and a count of how many it has had; a focus session credits the task it was started for. Tasks can be edited and deleted from their ⋯ menu, ticked off, reopened, and are grouped into *Completed today* and *Completed earlier*. Estimating more than four sessions gets a gentle suggestion to split the work. A focus interval cannot start without a task selected, so every session is attributed to something.

**Interruptions.** Ctrl+I opens a one-line capture for whatever just came up — an email to send, a thought, a knock at the door — without touching the timer. Each note is tagged *internal* or *external* and lands in the interruption inbox for after the interval, where it can be marked handled, turned into a task, or deleted.

**Desktop notification capture.** Optionally, Pomodoro watches the desktop notification service and files a copy of what other applications send, so that a message which arrived mid-focus can be read afterwards instead of during. Off by default; see [Desktop notification capture](#desktop-notification-capture).

**Silence during focus.** Optionally, Pomodoro switches GNOME's own Do Not Disturb on for the length of each focus interval and back off at the end.

**The day's ledger.** Under the timer: how many sessions are planned across today's tasks against a full-day capacity, how many focus sessions are complete and how many minutes that is, how many interruptions were captured, and every session of the day with its outcome. It rolls over at midnight even when the window has been open since yesterday, and the toolbar shows the same totals at a glance.

**Sound and notifications.** When an interval runs out, Pomodoro plays your sound theme's "alarm clock elapsed" sound and shows a notification; when you tick a task off, it plays the theme's short "complete" chime and shows a notification naming the task. Both work with the window closed to the tray. See [Sound and notifications](#sound-and-notifications).

**The tray.** Closing the window hides it; the tray menu shows it again, starts, pauses or resumes the timer — the item says which — skips the current interval, or quits. Only one copy of Pomodoro runs at a time; launching it again brings the existing window forward.

**Light and dark.** Follows the desktop's appearance setting live, or stays on the theme you pick, from the toolbar button or Settings → Appearance. Keyboard navigation throughout, high-contrast and reduced-motion support.

## A day with it

1. Add today's tasks with an estimate each; the ledger shows how the plan compares with a full day.
2. Select a task and start a focus.
3. When something comes up, press Ctrl+I, note it, and go back to work. If desktop notification capture is on, whatever other apps sent is waiting in the inbox too.
4. When the focus ends you hear it and see it; take the break.
5. During the break, go through the interruption inbox: handle, turn into a task, or delete.
6. After four focus sessions, take the long break.
7. At the end of the day, the ledger is the record of what was planned, what was done and what got in the way.

### Keyboard

| Key | Does |
|---|---|
| Space | Start, pause or resume the timer |
| Ctrl+I | Capture an interruption |
| Ctrl+N | Add a task |
| Ctrl+1 / Ctrl+2 / Ctrl+3 | Choose focus, short break or long break (while idle) |
| Ctrl+, | Open Settings |
| Escape | Close whatever is innermost: an open menu, then a dialog, then the task drawer |

Space belongs to whatever control has focus: tabbing to a button and pressing Space presses that button, not the timer. While a dialog is open, everything behind it is inert.

### Tasks in detail

The ⋯ menu on a task has **Edit** — title and estimate — and **Delete**. Several tasks can be open for editing at once, and a change the app refuses (an empty title, say) leaves the form open with what you typed. Ticking a task moves it under **Completed today**; tasks finished on an earlier day sit under **Completed earlier**. Either can be reopened, or deleted with the bin icon, which asks once first. Deleting a task removes its session count; the sessions already recorded in the ledger stay.

### Intervals in detail

Skipping a focus throws its elapsed time away, so the Skip button asks once first; skipping a break is a single click. Reset puts the current interval back to its full length; if it had already started, the ledger records it as abandoned. A running focus locks task selection, so a session cannot change owner halfway through.

Quitting from the tray pauses a running interval rather than letting it run out while the app is closed: the next launch shows it paused with the time it had left, and no session is recorded for time you were not there for. An interval that ran out while Pomodoro was not running at all — after a crash, say — is settled quietly at the next launch: the session is recorded as ending at its deadline, with no sound and no notification, and the next interval is left for you to start.

### Sound and notifications

Both can be switched off separately under Settings → Alerts. **Test sound** there plays the interval sound so you can hear it before relying on it, and tells you if nothing on the machine could play it.

The sound is played by Pomodoro itself rather than attached to the notification, for two reasons: it has to work with the window closed to the tray, and GNOME's Do Not Disturb — which Pomodoro can switch on for you during focus — silences a notification's own sound along with its banner. It comes from your sound theme, through `canberra-gtk-play` (part of every standard Ubuntu desktop), which follows the theme's inheritance the way the desktop does; failing that, the theme's file is played with `pw-play`, `gst-play-1.0` or `paplay`. If none of those is available the interval still ends and the notification still shows; there is just no sound. The `.deb` recommends, rather than requires, a player and the `freedesktop` sound theme.

An interval's alarm is never lost to a lesser sound: if it falls due while a task's chime is still playing, it plays next. Pomodoro's Sound setting is the only switch; it is not silenced by turning off the desktop's general "event sounds". Skipping or resetting an interval is not finishing one, so those are silent.

### Theme

The button beside the settings gear steps through **System**, **Light** and **Dark** and saves at once. On System, Pomodoro follows the desktop's appearance setting — GNOME's Dark style switch — and repaints while it is open, without a restart. The last theme used is applied before the window first paints, so launching never flashes the wrong one.

## Desktop notification capture

Pomodoro can watch the desktop notification service and file a copy of what other applications send, so it can be read after a focus interval instead of during one. It is **off by default** and does nothing until you turn it on in Settings → Notification capture.

What it does and does not do:

- It **files a copy**. Filing is passive: Pomodoro observes the notification after the desktop has already accepted it.
- It **does not hide the banner**. Watching a notification cannot stop it. Banners still appear and sounds still play.
- **Silence banners during focus** is the setting that actually quiets the desktop. It switches GNOME's own Do Not Disturb on for the length of each focus interval and switches it back at the end. It is a separate, off-by-default toggle because it changes a setting that belongs to the desktop rather than to Pomodoro. Banners you had already turned off yourself are left alone, quitting from the tray puts the setting back, and if Pomodoro is killed mid-interval the next launch repairs it.

You choose what is worth keeping: a minimum urgency, whether to capture only during focus, a list of muted apps, and a list of priority apps that bypass the other rules. Pomodoro never captures its own notifications. A notification a sender updates in place — a download counting up, a call still ringing — stays one row in the inbox rather than one per update.

The inbox shows each captured notification with its app, urgency, time and whether it arrived during focus. Each can be marked triaged (or moved back to pending), turned into a task, or deleted. Deleting removes the captured copy including its text; a task already made from it stays.

Captured summaries and bodies routinely contain message text and one-time codes. They are written only to the local data file below, which Pomodoro keeps owner-only (`0600`, in a `0700` directory). They are never logged and never leave the machine. Settings → Data has an explicit, confirmed action for deleting every captured copy.

Capture needs a session bus that will hand out a monitor connection, which is the case on a standard Ubuntu GNOME desktop. If it cannot start, Settings and the inbox say so rather than showing an empty list as though nothing had arrived.

## Settings

| Section | What is there |
|---|---|
| Timing | Focus, short break and long break lengths; rounds before a long break |
| Flow | Start breaks automatically; start the next focus automatically |
| Alerts | Desktop notifications; sound; Test sound |
| Notification capture | The capture switch and its filters; silence banners during focus |
| Appearance | Theme; timer face |
| Data | Clear session history; delete captured notifications — each asks first |
| Keyboard | The shortcuts above |

Edits belong to you until you press Save: a state change arriving from the timer mid-edit does not overwrite what you were typing, a save the app refuses leaves the dialog open with your edits, and Cancel discards them.

## Install on Ubuntu

Build or obtain the Debian package and install it:

    sudo apt install ./src-tauri/target/release/bundle/deb/Pomodoro_0.1.0_amd64.deb

Or, without administrator rights, install it for your user alone. The package's files go under your home directory, and the launcher entry points at them:

    dpkg-deb -x Pomodoro_0.1.0_amd64.deb /tmp/pomodoro-pkg
    install -Dm755 /tmp/pomodoro-pkg/usr/bin/pomodoro ~/.local/bin/pomodoro
    cp -r /tmp/pomodoro-pkg/usr/share/icons ~/.local/share/
    sed "s|^Exec=pomodoro$|Exec=$HOME/.local/bin/pomodoro|" \
        /tmp/pomodoro-pkg/usr/share/applications/Pomodoro.desktop \
        > ~/.local/share/applications/Pomodoro.desktop
    update-desktop-database ~/.local/share/applications
    rm -r /tmp/pomodoro-pkg

To remove a per-user install, delete those files. Do not keep both kinds of install at once.

Open Pomodoro from the application launcher. Closing the window hides it in the system tray so an active timer can continue; choose Quit from the tray menu to exit completely.

The package was built on Ubuntu 24.04 x86_64 and is directly verified for Ubuntu 24.04 or newer. To support Ubuntu 22.04 with the widest binary compatibility, build the release on Ubuntu 22.04.

## Local data

Application data is stored beneath the standard Linux user data directory, normally:

    ~/.local/share/app.pomodoro.timer/pomodoro.json

The file holds captured notification text, so Pomodoro restricts it to its owner (`0600`) inside an owner-only directory (`0700`) on every save. A store written by an earlier build is tightened the next time the app saves. Session history is kept to the most recent 5,000 records.

If the file cannot be read at launch — truncated by a full disk, or edited by hand into something that will not parse — Pomodoro starts fresh and says so in the window. It does not save over the old file: it is kept beside `pomodoro.json` as `pomodoro.unreadable-<timestamp>.json`, owner-only like the store itself, so it can be repaired or mined by hand. If it cannot be moved aside either — a data folder that has become read-only, say — Pomodoro leaves it exactly where it is and saves nothing for that session, and the notice says so.

Removing the application does not remove this data file.

### Upgrading from the Kipindi build

This app was previously named Kipindi and used the bundle identifier `app.kipindi.timer`, which put its data at a different path. If you ran that build, carry your tasks, settings, and session history across once:

    mkdir -p ~/.local/share/app.pomodoro.timer
    cp ~/.local/share/app.kipindi.timer/kipindi.json \
       ~/.local/share/app.pomodoro.timer/pomodoro.json

The old directory is left in place. Remove it once the new build has started cleanly and your history looks right.

## Develop

Pomodoro is a [Tauri 2](https://tauri.app) application: a Rust backend (`src-tauri/`) that owns the timer, the store, the tray, notification capture and sound, and a React front end (`src/`) that draws it. The window counts the timer down locally from a deadline the backend gives it; the backend only speaks when something other than the clock changes.

Required Ubuntu packages:

    sudo apt install libwebkit2gtk-4.1-dev build-essential file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

Install project dependencies and run the checks. These are the same steps CI runs on every pull request, so running all of them first avoids a red build:

    npm install
    npm run build
    npm test
    cd src-tauri
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo test

No test plays a sound or sends a real notification. Some checks need a real GNOME session — a session bus, a notification daemon, and `gsettings` — so they are marked `#[ignore]` and skipped by CI. Run them on an Ubuntu desktop, from `src-tauri`:

    cargo test -- --ignored --nocapture

They cover capture against the live bus, a notification arriving mid-focus being filed without disturbing the timer, Pomodoro's own alerts staying out of the inbox, banners being silenced and restored, and carrying a real `app.kipindi.timer` store across. None of them write to your live Pomodoro data or leave the desktop's banner setting changed.

Run the desktop app in development:

    npm run tauri dev

Opening `npm run dev` in a browser shows the interface with sample data and no backend. The theme button works there; everything that saves does not.

`public/theme-init.js` is the one script outside the bundle. It applies the remembered theme before the first paint, and it is a file rather than an inline script because the content security policy only allows the app's own scripts.

Build a Debian package:

    npm run tauri build -- --bundles deb
