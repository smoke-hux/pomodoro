use super::*;
use crate::sound::Cue;
use std::collections::HashSet;

const SYSTEM_SOUNDS: &str = "/usr/share/sounds";

fn system_sounds() -> Vec<PathBuf> {
    vec![PathBuf::from(SYSTEM_SOUNDS)]
}

/// The file for a cue's own id, in a theme that inherits from nothing.
fn find_sound_file(
    cue: Cue,
    theme: Option<&str>,
    data_home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    find_event_file(
        cue.event_id(),
        &theme_chain(theme, |_| None),
        data_home,
        &system_sounds(),
        &exists,
    )
}

fn only(paths: &[&str]) -> impl Fn(&Path) -> bool {
    let present: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
    move |path| present.contains(path)
}
#[test]
fn the_users_theme_is_searched_before_the_systems_and_the_stock_themes_last() {
    let home = Path::new("/home/someone/.local/share");
    let everywhere = [
        "/home/someone/.local/share/sounds/Ocean/complete.oga",
        "/home/someone/.local/share/sounds/Ocean/stereo/complete.oga",
        "/usr/share/sounds/Ocean/stereo/complete.oga",
        "/usr/share/sounds/Yaru/stereo/complete.oga",
        "/usr/share/sounds/freedesktop/stereo/complete.oga",
    ];
    // Each is found once everything ahead of it is gone.
    for (index, expected) in everywhere.iter().enumerate() {
        let found = find_sound_file(
            Cue::TaskDone,
            Some("Ocean"),
            Some(home),
            only(&everywhere[index..]),
        );
        assert_eq!(found, Some(PathBuf::from(expected)));
    }
    assert_eq!(
        find_sound_file(Cue::TaskDone, Some("Ocean"), Some(home), only(&[])),
        None
    );
}

#[test]
fn a_custom_theme_keeps_its_files_directly_in_the_theme_directory() {
    let found = find_sound_file(
        Cue::TaskDone,
        Some("__custom"),
        Some(Path::new("/home/someone/.local/share")),
        only(&[
            "/home/someone/.local/share/sounds/__custom/complete.ogg",
            "/usr/share/sounds/freedesktop/stereo/complete.oga",
        ]),
    );
    assert_eq!(
        found,
        Some(PathBuf::from(
            "/home/someone/.local/share/sounds/__custom/complete.ogg"
        ))
    );
}

#[test]
fn without_a_theme_or_a_data_home_the_stock_themes_are_still_searched() {
    let stock = "/usr/share/sounds/freedesktop/stereo/complete.oga";
    assert_eq!(
        find_sound_file(Cue::TaskDone, None, None, only(&[stock])),
        Some(PathBuf::from(stock))
    );
    // A theme with no data home still has its system directory.
    let system = "/usr/share/sounds/Ocean/stereo/complete.oga";
    assert_eq!(
        find_sound_file(Cue::TaskDone, Some("Ocean"), None, only(&[system, stock])),
        Some(PathBuf::from(system))
    );
}

#[test]
fn within_a_directory_oga_comes_before_ogg_and_ogg_before_wav() {
    let directory = "/usr/share/sounds/freedesktop/stereo";
    let all = [
        format!("{directory}/complete.oga"),
        format!("{directory}/complete.ogg"),
        format!("{directory}/complete.wav"),
    ];
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    for index in 0..all.len() {
        assert_eq!(
            find_sound_file(Cue::TaskDone, None, None, only(&all[index..])),
            Some(PathBuf::from(all[index]))
        );
    }
    // The directory counts for more than the extension.
    let yaru_wav = "/usr/share/sounds/Yaru/stereo/complete.wav";
    assert_eq!(
        find_sound_file(Cue::TaskDone, None, None, only(&[yaru_wav, all[0]])),
        Some(PathBuf::from(yaru_wav))
    );
}

#[test]
fn a_theme_name_that_could_leave_the_sounds_directory_is_not_used() {
    let home = Path::new("/home/someone/.local/share");
    for name in ["../../../etc", "..", "a/b", "/etc", "Yaru/../..", ""] {
        let asked = std::cell::RefCell::new(Vec::new());
        let found = find_sound_file(Cue::TaskDone, Some(name), Some(home), |path| {
            asked.borrow_mut().push(path.to_path_buf());
            false
        });
        assert_eq!(found, None);
        let asked = asked.into_inner();
        // Only the stock themes are looked for — in the user's own copy of
        // each and in the system's — and the bad name appears nowhere.
        assert_eq!(asked.len(), 2 * 3 * EXTENSIONS.len(), "{name:?}: {asked:?}");
        assert!(
            asked.iter().all(|path| {
                ["Yaru", "freedesktop"].iter().any(|stock| {
                    path.starts_with(home.join("sounds").join(stock))
                        || path.starts_with(Path::new(SYSTEM_SOUNDS).join(stock))
                })
            }),
            "{name:?} was looked for: {asked:?}"
        );
    }
}

#[test]
fn the_theme_name_is_what_gsettings_printed_without_its_quotes() {
    assert_eq!(parse_theme_name("'Yaru'\n"), Some("Yaru".to_string()));
    assert_eq!(
        parse_theme_name("'__custom'\n"),
        Some("__custom".to_string())
    );
    assert_eq!(parse_theme_name("\"it's\"\n"), Some("it's".to_string()));
    assert_eq!(parse_theme_name("''\n"), None);
    assert_eq!(parse_theme_name(""), None);
    assert_eq!(parse_theme_name("'../../etc'\n"), None);
    assert_eq!(parse_theme_name("'a/b'\n"), None);
}

#[test]
fn the_data_home_is_xdg_data_home_or_dot_local_share() {
    let os = |text: &str| Some(OsString::from(text));
    assert_eq!(
        data_home_from(os("/data"), os("/home/someone")),
        Some(PathBuf::from("/data"))
    );
    assert_eq!(
        data_home_from(None, os("/home/someone")),
        Some(PathBuf::from("/home/someone/.local/share"))
    );
    // Relative and empty values are ignored, as the specification says.
    assert_eq!(
        data_home_from(os("data"), os("/home/someone")),
        Some(PathBuf::from("/home/someone/.local/share"))
    );
    assert_eq!(
        data_home_from(os(""), os("/home/someone")),
        Some(PathBuf::from("/home/someone/.local/share"))
    );
    assert_eq!(data_home_from(None, None), None);
    assert_eq!(data_home_from(None, os("somewhere")), None);
}

#[test]
fn the_system_sounds_are_in_each_of_xdg_data_dirs() {
    let os = |text: &str| Some(OsString::from(text));
    let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
    assert_eq!(
        system_sounds_from(os("/run/current-system/sw/share:/usr/share")),
        paths(&["/run/current-system/sw/share/sounds", "/usr/share/sounds"])
    );
    // Unset or empty is the specification's default; relative entries and
    // repeats are left out.
    let default = paths(&["/usr/local/share/sounds", "/usr/share/sounds"]);
    assert_eq!(system_sounds_from(None), default);
    assert_eq!(system_sounds_from(os("")), default);
    assert_eq!(
        system_sounds_from(os("share:/usr/share::/usr/share")),
        paths(&["/usr/share/sounds"])
    );

    // A theme that lives only outside /usr/share is found.
    let nix = "/run/current-system/sw/share/sounds/freedesktop/stereo/complete.oga";
    assert_eq!(
        find_event_file(
            "complete",
            &theme_chain(None, |_| None),
            None,
            &system_sounds_from(os("/run/current-system/sw/share:/usr/share")),
            &only(&[nix])
        ),
        Some(PathBuf::from(nix))
    );
}

#[test]
fn a_theme_is_searched_along_what_it_inherits_from_and_the_stock_themes_last() {
    let index = |theme: &str| match theme {
        "Mine" => {
            Some("[Sound Theme]\nName=Mine\nInherits=Ocean, Deep\nDirectories=.\n".to_string())
        }
        "Ocean" => Some("Inherits = Yaru".to_string()),
        // A loop, and a name that would climb out of the sounds directory.
        "Deep" => Some("Inherits=Mine,../../etc".to_string()),
        _ => None,
    };
    assert_eq!(
        theme_chain(Some("Mine"), index),
        ["Mine", "Ocean", "Deep", "Yaru", "freedesktop"]
    );
    assert_eq!(theme_chain(None, |_| None), ["Yaru", "freedesktop"]);
    assert_eq!(theme_chain(Some("../x"), |_| None), ["Yaru", "freedesktop"]);

    // A file only a grandparent theme holds is found. Searching just the
    // named theme and the stock ones missed it.
    let deep = "/usr/share/sounds/Deep/stereo/complete.oga";
    assert_eq!(
        find_event_file(
            "complete",
            &theme_chain(Some("Mine"), index),
            None,
            &system_sounds(),
            &only(&[deep])
        ),
        Some(PathBuf::from(deep))
    );
}
