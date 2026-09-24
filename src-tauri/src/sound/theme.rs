//! Desktop theme discovery and freedesktop inheritance/file lookup.

use crate::system;
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "org.gnome.desktop.sound";
const KEY: &str = "theme-name";

/// Where themes live when they are not the user's own, if `XDG_DATA_DIRS`
/// does not say: the default the base directory specification gives it.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";
/// Tried in this order within each directory. `.oga` is what the stock themes
/// ship; the other two are what the sound theme specification also allows.
const EXTENSIONS: [&str; 3] = ["oga", "ogg", "wav"];
/// Themes searched after the user's own chain: Ubuntu's default, which may hold
/// an id the user's theme does not, and the one every theme ends at.
const STOCK_THEMES: [&str; 2] = ["Yaru", "freedesktop"];
/// An `Inherits` chain longer than this is a loop or a mistake.
const MAX_THEMES: usize = 8;

/// A theme name that can be used as one directory name and nothing else.
///
/// The name comes out of a desktop setting or a theme's index file, which
/// anything running as the user can write. It is about to be joined onto a
/// path, so one that could climb out of the sounds directory is treated as no
/// theme at all.
fn safe_theme_name(name: &str) -> Option<&str> {
    (!name.is_empty() && !name.contains('/') && !name.contains("..")).then_some(name)
}

/// The theme name out of what `gsettings get` printed: a GVariant string, in
/// quotes, on a line of its own.
fn parse_theme_name(printed: &str) -> Option<String> {
    let printed = printed.trim();
    let bare = ['\'', '"']
        .into_iter()
        .find_map(|quote| printed.strip_prefix(quote)?.strip_suffix(quote))
        .unwrap_or(printed);
    safe_theme_name(bare).map(str::to_string)
}

/// The themes a theme's `index.theme` says it inherits from.
fn inherits(index: &str) -> Vec<String> {
    index
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Inherits"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .flat_map(|names| names.split(','))
        .filter_map(|name| safe_theme_name(name.trim()))
        .map(str::to_string)
        .collect()
}

/// The themes to search, in order: the user's, what it inherits from, what
/// those inherit from, and the stock themes last.
///
/// `read_index` is asked for a theme's `index.theme` instead of the disk, so
/// the order can be tested without one. Following `Inherits` is what the theme
/// player does; searching only the named theme and the stock ones found
/// nothing for a theme built on top of some third one.
fn theme_chain(theme: Option<&str>, read_index: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut next: Vec<String> = theme
        .and_then(safe_theme_name)
        .map(str::to_string)
        .into_iter()
        .collect();
    while let Some(name) = next.first().cloned() {
        next.remove(0);
        if chain.contains(&name) || chain.len() >= MAX_THEMES {
            continue;
        }
        if let Some(index) = read_index(&name) {
            next.extend(inherits(&index));
        }
        chain.push(name);
    }
    for stock in STOCK_THEMES {
        if !chain.iter().any(|name| name == stock) {
            chain.push(stock.to_string());
        }
    }
    chain
}

/// The first file that exists for one event id.
///
/// For each theme the user's own copy comes before the system's, as it does
/// for libcanberra. Its files are looked for directly in the theme directory
/// first: that is how GNOME lays out `__custom`, the theme it writes when an
/// alert sound is picked in Settings.
fn find_event_file(
    event_id: &str,
    themes: &[String],
    data_home: Option<&Path>,
    system_sounds: &[PathBuf],
    exists: &impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let mut directories = Vec::new();
    for theme in themes {
        if let Some(data_home) = data_home {
            let own = data_home.join("sounds").join(theme);
            directories.push(own.clone());
            directories.push(own.join("stereo"));
        }
        for system in system_sounds {
            directories.push(system.join(theme).join("stereo"));
        }
    }

    directories
        .iter()
        .flat_map(|directory| {
            EXTENSIONS
                .iter()
                .map(move |extension| directory.join(format!("{event_id}.{extension}")))
        })
        .find(|path| exists(path))
}

/// Where the user's own data lives: `XDG_DATA_HOME`, or `~/.local/share` when
/// that is unset. The specification says a relative value is to be ignored.
fn data_home_from(xdg_data_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute = |value: OsString| Some(PathBuf::from(value)).filter(|path| path.is_absolute());
    xdg_data_home
        .and_then(absolute)
        .or_else(|| Some(home.and_then(absolute)?.join(".local").join("share")))
}

/// The system's sound directories, most important first: `sounds` in each
/// directory of `XDG_DATA_DIRS`, as libcanberra searches them. Not every system
/// keeps its themes under `/usr/share` — NixOS, Guix and a theme installed to
/// `/usr/local` do not. The specification says an unset or empty value means
/// the default, and a relative entry is to be ignored.
fn system_sounds_from(xdg_data_dirs: Option<OsString>) -> Vec<PathBuf> {
    let dirs = xdg_data_dirs
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    let mut sounds: Vec<PathBuf> = Vec::new();
    for dir in env::split_paths(&dirs).filter(|dir| dir.is_absolute()) {
        let dir = dir.join("sounds");
        if !sounds.contains(&dir) {
            sounds.push(dir);
        }
    }
    sounds
}

/// The sound theme the desktop is set to, or `None` if that cannot be learned:
/// no `gsettings`, no GNOME schema, a name that is not safe to use. The search
/// then goes straight to the themes every machine has.
fn desktop_theme_name() -> Option<String> {
    let mut command = system::command("gsettings");
    command.args(["get", SCHEMA, KEY]);
    let answer = system::ask(command)?;
    answer
        .status
        .success()
        .then(|| parse_theme_name(&answer.stdout))?
}

/// Searches the themes on this machine for one event's file. The theme chain
/// is worked out on the first call and kept for the second.
pub(super) fn file_finder() -> impl FnMut(&str) -> Option<PathBuf> {
    let mut themes: Option<(Vec<String>, Option<PathBuf>, Vec<PathBuf>)> = None;
    move |event_id| {
        let (themes, data_home, system_sounds) = themes.get_or_insert_with(|| {
            let data_home = data_home_from(env::var_os("XDG_DATA_HOME"), env::var_os("HOME"));
            let system_sounds = system_sounds_from(env::var_os("XDG_DATA_DIRS"));
            let read_index = |theme: &str| {
                data_home
                    .iter()
                    .map(|home| home.join("sounds"))
                    .chain(system_sounds.iter().cloned())
                    .find_map(|sounds| {
                        fs::read_to_string(sounds.join(theme).join("index.theme")).ok()
                    })
            };
            let themes = theme_chain(desktop_theme_name().as_deref(), read_index);
            (themes, data_home, system_sounds)
        });
        find_event_file(
            event_id,
            themes,
            data_home.as_deref(),
            system_sounds,
            &|path: &Path| path.is_file(),
        )
    }
}

#[cfg(test)]
mod tests;
