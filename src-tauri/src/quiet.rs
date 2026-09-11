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
//! Every failure here is soft. A machine without `gsettings`, without GNOME, or
//! with the schema locked down simply does not go quiet; the timer and capture
//! are unaffected.

use std::process::Command;

const SCHEMA: &str = "org.gnome.desktop.notifications";
const KEY: &str = "show-banners";

/// Reads the desktop's current banner setting.
///
/// `None` means the question could not be answered — no `gsettings`, no GNOME
/// schema, an unreadable value — which callers treat as "do not touch it".
pub fn read_show_banners() -> Option<bool> {
    let output = Command::new("gsettings")
        .args(["get", SCHEMA, KEY])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Writes the desktop's banner setting. The error is the reason, never anything
/// drawn from a notification.
pub fn write_show_banners(value: bool) -> Result<(), String> {
    let output = Command::new("gsettings")
        .args(["set", SCHEMA, KEY, if value { "true" } else { "false" }])
        .output()
        .map_err(|error| format!("could not run gsettings: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "gsettings refused to set {SCHEMA} {KEY}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}
