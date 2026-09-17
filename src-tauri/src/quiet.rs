//! Turns the desktop's notification banners off for the length of a focus
//! interval.
//!
//! Watching the bus files a copy of every notification, but it cannot stop one:
//! a monitor is a passive observer, and the banner has already been drawn by the
//! time the call is seen. Actually going quiet means asking the desktop, and on
//! GNOME — Ubuntu's default — that is the `show-banners` key the shell's own
//! "Do Not Disturb" switch writes.
//!
//! The key belongs to the desktop, not to Pomodoro, so this is off by default,
//! the previous value is recorded before it is touched, and it is put back at
//! the end of the interval. A value the user had already set to `false` is left
//! alone: there is nothing to restore, and nothing to take credit for.
//!
//! `gsettings` is run through [`crate::system`]: with the AppImage taken out of
//! its environment, without which it looks for the desktop's keys in the
//! bundle's schemas and this whole feature silently did nothing in that build;
//! and with a deadline, because these calls are made with the data locked.
//!
//! Every failure here is soft. A machine without `gsettings`, without GNOME, or
//! with the schema locked down simply does not go quiet; the timer and capture
//! are unaffected.

use crate::system;

const SCHEMA: &str = "org.gnome.desktop.notifications";
const KEY: &str = "show-banners";

/// Reads the desktop's current banner setting.
///
/// `None` means the question could not be answered — no `gsettings`, no GNOME
/// schema, an unreadable value — which callers treat as "do not touch it".
pub fn read_show_banners() -> Option<bool> {
    let mut command = system::command("gsettings");
    command.args(["get", SCHEMA, KEY]);
    let answer = system::ask(command)?;
    if !answer.status.success() {
        return None;
    }
    match answer.stdout.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Writes the desktop's banner setting. The error is the reason, never anything
/// drawn from a notification.
pub fn write_show_banners(value: bool) -> Result<(), String> {
    let mut command = system::command("gsettings");
    command.args(["set", SCHEMA, KEY, if value { "true" } else { "false" }]);
    let answer = system::ask(command)
        .ok_or_else(|| "gsettings could not be run, or did not answer in time".to_string())?;
    if answer.status.success() {
        return Ok(());
    }
    Err(format!(
        "gsettings refused to set {SCHEMA} {KEY}: {}",
        answer.stderr.trim()
    ))
}
