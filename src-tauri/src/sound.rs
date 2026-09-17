//! Plays a sound from the desktop's sound theme when an interval runs out and
//! when a task is marked done.
//!
//! Pomodoro ships no audio and links no audio library. The desktop already has
//! a sound theme, the user may have chosen it, and a focus timer that brings
//! its own chime is one more thing that does not sound like the rest of the
//! machine. So the sound is an event id — `alarm-clock-elapsed`, `complete` —
//! handed to `canberra-gtk-play`, the one player that looks an id up in the
//! user's theme and follows its `Inherits` chain down to `freedesktop`. Where
//! that player is missing or refuses, the theme is searched by hand for the
//! file and a plain file player is tried instead: `pw-play`, `gst-play-1.0`,
//! `paplay`, in that order. Never `aplay`, which cannot decode the Ogg Vorbis
//! the themes are made of and would play it as noise.
//!
//! The spawned player is the only source of sound. The notifications Pomodoro
//! sends carry no sound hint: GNOME would play it as well, twice over, and Do
//! Not Disturb — which Pomodoro itself can switch on for a focus — swallows a
//! notification's sound along with its banner. The end of that very focus is
//! the one moment the sound matters most.
//!
//! The desktop-wide "event sounds" switch is overridden per event, with
//! `canberra.enable=1`. Pomodoro has its own Sound setting and that is the one
//! control; a switch elsewhere silently defeating a setting the user turned on
//! here reads as a bug, not as a preference.
//!
//! [`play`] never blocks its caller, which may be the timer's reconciliation
//! thread or the main thread: everything, including asking `gsettings` for the
//! theme, happens on a short-lived thread of its own. Every child is waited
//! for, because this is a tray application that runs for days and a child that
//! is dropped unwaited stays a zombie for all of them. A player that hangs —
//! an audio server with no usable sink will do it — is killed at a deadline.
//!
//! Every failure here is soft. A machine with no player, no theme or no audio
//! at all stays silent and says so once on stderr; the timer, the store and
//! the notifications are unaffected. Nothing is ever synthesised in place of
//! the theme's sound.

use std::{
    env,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const SCHEMA: &str = "org.gnome.desktop.sound";
const KEY: &str = "theme-name";

/// How long one player may run. The longest sound played here is about six
/// seconds; a player still going at fifteen is not playing, it is stuck.
const PLAYER_DEADLINE: Duration = Duration::from_secs(15);
/// How often a running player is looked in on. Only the noticing of its end
/// waits on this, never the start of the sound.
const PLAYER_POLL: Duration = Duration::from_millis(100);
/// `gsettings` answers in a few milliseconds or not at all. It runs before
/// the sound, so it is looked in on more often and given up on much sooner.
const GSETTINGS_DEADLINE: Duration = Duration::from_secs(2);
const GSETTINGS_POLL: Duration = Duration::from_millis(10);

/// Where themes live when they are not the user's own.
const SYSTEM_SOUNDS: &str = "/usr/share/sounds";
/// Tried in this order within each directory. `.oga` is what the stock themes
/// ship; the other two are what the sound theme specification also allows.
const EXTENSIONS: [&str; 3] = ["oga", "ogg", "wav"];

/// Something worth a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// A focus or a break ran its full time. Skipping or resetting one is not
    /// this.
    IntervalFinished,
    /// A task went from not done to done.
    TaskDone,
}

impl Cue {
    /// The id in the freedesktop sound naming specification.
    /// `alarm-clock-elapsed` is defined there as "a user configured alarm
    /// elapsed", which is exactly what the end of an interval is.
    fn event_id(self) -> &'static str {
        match self {
            Cue::IntervalFinished => "alarm-clock-elapsed",
            Cue::TaskDone => "complete",
        }
    }

    /// What the sound is, for the audio server's own listing of what is
    /// playing. Fixed text: nothing of the user's goes on a command line.
    fn description(self) -> &'static str {
        match self {
            Cue::IntervalFinished => "Interval finished",
            Cue::TaskDone => "Task complete",
        }
    }
}

/// One way of making the sound: a program and its arguments.
///
/// The program is a bare name, found through `PATH` like any other command. No
/// player has one fixed home across distributions, and a bare name is also
/// what lets a test put a harmless stand-in in front of the real thing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    program: &'static str,
    args: Vec<OsString>,
}

impl Candidate {
    fn new<I, A>(program: &'static str, args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        Self {
            program,
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// The ways of playing `cue`, best first.
///
/// `canberra-gtk-play` leads because it is the only one that takes an event id
/// and resolves it in the user's theme; the rest need to be handed a `file`,
/// and are left out when the theme search found none. `pw-play` is told the
/// stream is a notification so the audio server routes and ducks it as one.
/// `gst-play-1.0` reads its keyboard controls from stdin unless told not to.
/// `paplay` is not part of a default install any more and costs nothing to try
/// last.
fn candidates(cue: Cue, file: Option<&Path>) -> Vec<Candidate> {
    let mut list = vec![Candidate::new(
        "canberra-gtk-play",
        [
            "-i",
            cue.event_id(),
            "-d",
            cue.description(),
            "--property=canberra.enable=1",
        ],
    )];
    if let Some(file) = file {
        let file = file.as_os_str();
        list.push(Candidate::new(
            "pw-play",
            [OsString::from("--media-role=Notification"), file.into()],
        ));
        list.push(Candidate::new(
            "gst-play-1.0",
            [
                OsString::from("--no-interactive"),
                OsString::from("-q"),
                file.into(),
            ],
        ));
        list.push(Candidate::new("paplay", [file]));
    }
    list
}

/// A theme name that can be used as one directory name and nothing else.
///
/// The name comes out of a desktop setting, which anything running as the user
/// can write. It is about to be joined onto a path, so one that could climb
/// out of the sounds directory is treated as no theme at all.
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

/// The first file that exists for one event id.
///
/// The user's own copy of the theme comes before the system's, as it does for
/// libcanberra. Its files are looked for directly in the theme directory
/// first: that is how GNOME lays out `__custom`, the theme it writes when an
/// alert sound is picked in Settings. Then the theme proper, then Yaru because
/// it is Ubuntu's default and may hold the id when the user's theme does not,
/// then `freedesktop`, which every theme inherits from in the end.
fn find_event_file(
    event_id: &str,
    theme: Option<&str>,
    data_home: Option<&Path>,
    exists: &impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let system = Path::new(SYSTEM_SOUNDS);
    let mut directories = Vec::new();
    if let Some(theme) = theme {
        if let Some(data_home) = data_home {
            let own = data_home.join("sounds").join(theme);
            directories.push(own.clone());
            directories.push(own.join("stereo"));
        }
        directories.push(system.join(theme).join("stereo"));
    }
    directories.push(system.join("Yaru").join("stereo"));
    directories.push(system.join("freedesktop").join("stereo"));

    directories
        .iter()
        .flat_map(|directory| {
            EXTENSIONS
                .iter()
                .map(move |extension| directory.join(format!("{event_id}.{extension}")))
        })
        .find(|path| exists(path))
}

/// The file the file players should be given for `cue`, if the themes on this
/// machine hold one.
///
/// `exists` is asked instead of the disk so the search order can be tested
/// without one. Yaru does not ship `alarm-clock-elapsed`, and a machine
/// without the `freedesktop` theme has it nowhere; the end of an interval then
/// borrows `complete`, because a short chime beats silence.
fn find_sound_file(
    cue: Cue,
    theme: Option<&str>,
    data_home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let theme = theme.and_then(safe_theme_name);
    find_event_file(cue.event_id(), theme, data_home, &exists).or_else(|| match cue {
        Cue::IntervalFinished => {
            find_event_file(Cue::TaskDone.event_id(), theme, data_home, &exists)
        }
        Cue::TaskDone => None,
    })
}

/// Where the user's own data lives: `XDG_DATA_HOME`, or `~/.local/share` when
/// that is unset. The specification says a relative value is to be ignored.
fn data_home_from(xdg_data_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute = |value: OsString| Some(PathBuf::from(value)).filter(|path| path.is_absolute());
    xdg_data_home
        .and_then(absolute)
        .or_else(|| Some(home.and_then(absolute)?.join(".local").join("share")))
}

/// Takes a child about to be spawned out of the AppImage it was launched from.
///
/// A Tauri AppImage points these variables at the libraries and GTK modules
/// bundled inside it. A system GTK binary that inherits them loads the wrong
/// libraries, or none, and dies before it has played anything; `gsettings`
/// sent to the bundle's schemas cannot find the desktop's. Best effort: this
/// is the list such an AppImage is known to export, not a guarantee that
/// nothing else leaks through.
fn leave_the_appimage(command: &mut Command, inside_appimage: bool) {
    const BUNDLE_ONLY: [&str; 7] = [
        "LD_LIBRARY_PATH",
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        "GDK_PIXBUF_MODULE_FILE",
        "GIO_MODULE_DIR",
        "GSETTINGS_SCHEMA_DIR",
    ];
    if inside_appimage {
        for name in BUNDLE_ONLY {
            command.env_remove(name);
        }
    }
}

/// What every child spawned here gets done to it first.
fn for_the_system(command: &mut Command) {
    leave_the_appimage(command, env::var_os("APPIMAGE").is_some());
}

/// Waits for `child` until `deadline` has passed, and reaps it either way.
///
/// `None` means it had to be killed, and so nothing is known about what it
/// did. A `try_wait` that fails is treated the same: whatever became of the
/// child, the one thing still owed is that it does not outlive this call as a
/// zombie.
fn wait_until(child: &mut Child, deadline: Duration, poll: Duration) -> Option<ExitStatus> {
    let began = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        let left = deadline.saturating_sub(began.elapsed());
        if left.is_zero() {
            break;
        }
        thread::sleep(poll.min(left));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

/// How a walk over the candidates ended. The index says which candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Walk {
    /// This one ran and reported success.
    Played(usize),
    /// This one was still running at the deadline and was killed.
    TimedOut(usize),
    /// None could be started, or every one that started reported failure.
    NoneWorked,
}

/// Tries each candidate in turn until one plays.
///
/// A program that cannot be started — not installed, not executable — and one
/// that exits with a failure both pass the turn to the next. One that runs
/// into the deadline does not: a player that hangs usually hangs after the
/// sound, waiting on an audio server that will not let go, and trying the next
/// would play it a second time. Even when it hung before the sound, the next
/// player is about to talk to the same server.
///
/// The children get no stdin, stdout or stderr. Nothing here reads them, and a
/// player that inherits a terminal will happily take keystrokes from it.
fn walk(
    candidates: &[Candidate],
    deadline: Duration,
    poll: Duration,
    prepare: impl Fn(&mut Command),
) -> Walk {
    for (index, candidate) in candidates.iter().enumerate() {
        let mut command = Command::new(candidate.program);
        command
            .args(&candidate.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        prepare(&mut command);
        let Ok(mut child) = command.spawn() else {
            continue;
        };
        match wait_until(&mut child, deadline, poll) {
            Some(status) if status.success() => return Walk::Played(index),
            Some(_) => {}
            None => return Walk::TimedOut(index),
        }
    }
    Walk::NoneWorked
}

/// The sound theme the desktop is set to, or `None` if that cannot be learned:
/// no `gsettings`, no GNOME schema, a name that is not safe to use. The search
/// then goes straight to the themes every machine has. Waited for with a
/// deadline like any other child, because the "a sound is playing" mark is
/// held while this runs and must not be held for ever.
fn desktop_theme_name() -> Option<String> {
    let mut command = Command::new("gsettings");
    command
        .args(["get", SCHEMA, KEY])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for_the_system(&mut command);
    let mut child = command.spawn().ok()?;
    // A theme name is far smaller than a pipe's buffer, so the child can
    // finish writing, and exit, before anything is read.
    let status = wait_until(&mut child, GSETTINGS_DEADLINE, GSETTINGS_POLL)?;
    if !status.success() {
        return None;
    }
    let mut printed = String::new();
    child.stdout.take()?.read_to_string(&mut printed).ok()?;
    parse_theme_name(&printed)
}

/// Set while a sound is being played, process-wide.
static PLAYING: AtomicBool = AtomicBool::new(false);

/// Holds the "a sound is playing" mark, and gives it back when dropped — at
/// the end of the playing thread, on any early way out of it, when the thread
/// could not be started at all and the closure that owns this is thrown away,
/// and during a panic's unwinding. A mark left set would silence Pomodoro
/// until it was restarted.
struct Playing(&'static AtomicBool);

impl Playing {
    /// Takes the mark, or returns `None` if somebody already holds it.
    fn claim(mark: &'static AtomicBool) -> Option<Self> {
        mark.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(mark))
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Plays the theme's sound for `cue`, and returns at once.
///
/// A request made while another sound is still playing is dropped, not queued.
/// Two alarms on top of each other are worse than one, and the case that
/// really happens — the click that completes a task is also the first thing
/// to notice an interval has ended — wants the interval's sound alone.
///
/// Does nothing in a test build, so that no test, present or future, can make
/// a noise by reaching this through the code it is really about. The parts
/// are tested one by one below, the spawning with stand-in programs.
pub fn play(cue: Cue) {
    if cfg!(test) {
        return;
    }
    let Some(playing) = Playing::claim(&PLAYING) else {
        return;
    };
    let spawned = thread::Builder::new()
        .name("sound-player".to_string())
        .spawn(move || {
            let _playing = playing;
            let data_home = data_home_from(env::var_os("XDG_DATA_HOME"), env::var_os("HOME"));
            let file = find_sound_file(
                cue,
                desktop_theme_name().as_deref(),
                data_home.as_deref(),
                |path| path.is_file(),
            );
            let outcome = walk(
                &candidates(cue, file.as_deref()),
                PLAYER_DEADLINE,
                PLAYER_POLL,
                for_the_system,
            );
            if outcome == Walk::NoneWorked {
                eprintln!("could not play a sound: no system sound player worked");
            }
        });
    if let Err(error) = spawned {
        eprintln!("could not play a sound: {error}");
    }
}

/// Nothing in here makes a sound. The walk is exercised with `true`, `false`,
/// `sleep` and names that do not exist; where a real player's name has to be
/// the thing that is run, it is a logging shell script found through a `PATH`
/// that holds nothing else, and that `PATH` is proved to be in force before
/// the name is used.
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashSet,
        fs, io,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU32, Ordering},
    };

    const NOWHERE: &str = "pomodoro-no-such-player";

    fn untouched(_: &mut Command) {}

    fn stand_in(program: &'static str, args: &[&str]) -> Candidate {
        Candidate::new(program, args.iter().copied())
    }

    fn shown(candidate: &Candidate) -> Vec<String> {
        std::iter::once(candidate.program.to_string())
            .chain(
                candidate
                    .args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            )
            .collect()
    }

    fn only(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let present: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |path| present.contains(path)
    }

    #[test]
    fn each_cue_has_its_theme_id_and_a_description() {
        assert_eq!(Cue::IntervalFinished.event_id(), "alarm-clock-elapsed");
        assert_eq!(Cue::TaskDone.event_id(), "complete");
        assert_eq!(Cue::IntervalFinished.description(), "Interval finished");
        assert_eq!(Cue::TaskDone.description(), "Task complete");
    }

    #[test]
    fn the_theme_player_is_tried_first_then_the_file_players_in_order() {
        let file = Path::new("/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga");
        let list = candidates(Cue::IntervalFinished, Some(file));
        let list: Vec<Vec<String>> = list.iter().map(shown).collect();
        let file = file.to_str().unwrap();
        assert_eq!(
            list,
            [
                vec![
                    "canberra-gtk-play",
                    "-i",
                    "alarm-clock-elapsed",
                    "-d",
                    "Interval finished",
                    "--property=canberra.enable=1",
                ],
                vec!["pw-play", "--media-role=Notification", file],
                vec!["gst-play-1.0", "--no-interactive", "-q", file],
                vec!["paplay", file],
            ]
        );
    }

    #[test]
    fn with_no_file_found_only_the_theme_player_is_tried() {
        let list = candidates(Cue::TaskDone, None);
        assert_eq!(list.len(), 1);
        assert_eq!(
            shown(&list[0]),
            [
                "canberra-gtk-play",
                "-i",
                "complete",
                "-d",
                "Task complete",
                "--property=canberra.enable=1",
            ]
        );
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
    fn an_interval_with_no_alarm_sound_anywhere_borrows_the_completion_chime() {
        let alarm = "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga";
        let chime = "/usr/share/sounds/Yaru/stereo/complete.oga";
        // The alarm wins wherever it is, even behind a chime in a directory
        // that is searched earlier.
        assert_eq!(
            find_sound_file(Cue::IntervalFinished, None, None, only(&[alarm, chime])),
            Some(PathBuf::from(alarm))
        );
        assert_eq!(
            find_sound_file(Cue::IntervalFinished, None, None, only(&[chime])),
            Some(PathBuf::from(chime))
        );
        // It does not work the other way round.
        assert_eq!(
            find_sound_file(Cue::TaskDone, None, None, only(&[alarm])),
            None
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
            assert_eq!(asked.len(), 2 * EXTENSIONS.len(), "{name:?}: {asked:?}");
            assert!(
                asked.iter().all(|path| {
                    path.starts_with("/usr/share/sounds/Yaru/stereo")
                        || path.starts_with("/usr/share/sounds/freedesktop/stereo")
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
    fn a_child_of_an_appimage_is_not_handed_the_bundles_libraries() {
        let mut command = Command::new(NOWHERE);
        leave_the_appimage(&mut command, true);
        let removed: HashSet<String> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        let expected: HashSet<String> = [
            "LD_LIBRARY_PATH",
            "GTK_PATH",
            "GTK_EXE_PREFIX",
            "GTK_DATA_PREFIX",
            "GDK_PIXBUF_MODULE_FILE",
            "GIO_MODULE_DIR",
            "GSETTINGS_SCHEMA_DIR",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(removed, expected);

        // Outside one the environment is the user's own and is left alone.
        let mut command = Command::new(NOWHERE);
        leave_the_appimage(&mut command, false);
        assert_eq!(command.get_envs().count(), 0);
    }

    #[test]
    fn a_player_that_is_not_installed_passes_the_turn() {
        let list = [stand_in(NOWHERE, &[]), stand_in("true", &[])];
        assert_eq!(
            walk(
                &list,
                Duration::from_secs(10),
                Duration::from_millis(10),
                untouched
            ),
            Walk::Played(1)
        );
    }

    #[test]
    fn a_player_that_reports_failure_passes_the_turn() {
        let list = [stand_in("false", &[]), stand_in("true", &[])];
        assert_eq!(
            walk(
                &list,
                Duration::from_secs(10),
                Duration::from_millis(10),
                untouched
            ),
            Walk::Played(1)
        );
    }

    #[test]
    fn the_first_player_that_works_is_the_last_one_tried() {
        // If the walk went on past a success it would end on the second.
        let list = [stand_in("true", &[]), stand_in("true", &[])];
        assert_eq!(
            walk(
                &list,
                Duration::from_secs(10),
                Duration::from_millis(10),
                untouched
            ),
            Walk::Played(0)
        );
    }

    #[test]
    fn with_no_player_that_works_the_walk_says_so() {
        let list = [
            stand_in(NOWHERE, &[]),
            stand_in("pomodoro-no-such-player-either", &[]),
        ];
        assert_eq!(
            walk(
                &list,
                Duration::from_secs(10),
                Duration::from_millis(10),
                untouched
            ),
            Walk::NoneWorked
        );
        assert_eq!(
            walk(
                &[stand_in("false", &[])],
                Duration::from_secs(10),
                Duration::from_millis(10),
                untouched
            ),
            Walk::NoneWorked
        );
        assert_eq!(
            walk(
                &[],
                Duration::from_secs(10),
                Duration::from_millis(10),
                untouched
            ),
            Walk::NoneWorked
        );
    }

    /// This process's children that are running, or lying dead and unreaped,
    /// under the name `comm`.
    fn children_called(comm: &str) -> usize {
        let me = std::process::id().to_string();
        let Ok(entries) = fs::read_dir("/proc") else {
            return 0;
        };
        entries
            .flatten()
            .filter_map(|entry| fs::read_to_string(entry.path().join("stat")).ok())
            .filter(|stat| {
                // `pid (comm) state ppid ...`, and comm may itself hold a `)`.
                let Some((head, tail)) = stat.rsplit_once(") ") else {
                    return false;
                };
                head.split_once(" (").is_some_and(|(_, name)| name == comm)
                    && tail.split(' ').nth(1) == Some(me.as_str())
            })
            .count()
    }

    /// The only test here that runs `sleep`, which is what lets it count them.
    #[test]
    fn a_player_that_hangs_is_killed_and_reaped_and_nothing_else_is_tried() {
        let list = [stand_in("sleep", &["30"]), stand_in("true", &[])];
        let began = Instant::now();
        let outcome = walk(
            &list,
            Duration::from_millis(300),
            Duration::from_millis(20),
            untouched,
        );
        let took = began.elapsed();

        // Not `Played(1)`: the sound may already have been heard.
        assert_eq!(outcome, Walk::TimedOut(0));
        assert!(took >= Duration::from_millis(300), "gave up after {took:?}");
        assert!(
            took < Duration::from_secs(10),
            "waited {took:?} on a player that should have been killed"
        );
        if cfg!(target_os = "linux") {
            assert_eq!(children_called("sleep"), 0, "the player was left behind");
        }
    }

    #[test]
    fn a_second_sound_is_refused_while_the_first_is_playing() {
        static MARK: AtomicBool = AtomicBool::new(false);

        let first = Playing::claim(&MARK).expect("nothing is playing yet");
        assert!(Playing::claim(&MARK).is_none());
        assert!(Playing::claim(&MARK).is_none(), "a refusal changes nothing");
        drop(first);
        let again = Playing::claim(&MARK).expect("the first has finished");
        drop(again);
        assert!(!MARK.load(Ordering::Acquire));
    }

    #[test]
    fn the_mark_is_given_back_when_the_playing_thread_panics_or_never_starts() {
        static MARK: AtomicBool = AtomicBool::new(false);

        let playing = Playing::claim(&MARK).unwrap();
        let outcome = thread::spawn(move || {
            let _playing = playing;
            panic!("a panic on the playing thread, on purpose");
        })
        .join();
        assert!(outcome.is_err());
        assert!(Playing::claim(&MARK).is_some(), "the mark was left set");

        // A thread that could not be started drops the closure it was given.
        let playing = Playing::claim(&MARK).unwrap();
        let never_run = move || {
            let _playing = playing;
        };
        drop(never_run);
        assert!(Playing::claim(&MARK).is_some());
    }

    #[test]
    fn play_itself_does_nothing_under_test() {
        play(Cue::IntervalFinished);
        play(Cue::TaskDone);
        assert!(!PLAYING.load(Ordering::Acquire));
    }

    /// A directory of logging stand-ins, to be the whole of a child's `PATH`.
    struct StandIns(PathBuf);

    impl StandIns {
        fn new(name: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let directory = env::temp_dir().join(format!(
                "pomodoro-sound-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).unwrap();
            Self(directory)
        }

        /// Adds a script called `program` that writes its arguments, one to a
        /// line, beside itself and exits with `code`. Shell builtins only:
        /// with this directory as the whole `PATH` there is nothing else.
        fn add(&self, program: &str, code: i32) {
            let path = self.0.join(program);
            fs::write(
                &path,
                format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nexit {code}\n"),
            )
            .unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            // Another test forking at the wrong moment can hold this file open
            // for writing a little longer, and running it then is refused as
            // "text file busy". Run it until it is not, then clear the log.
            for _ in 0..200 {
                match Command::new(&path).stdin(Stdio::null()).status() {
                    Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    other => {
                        other.unwrap();
                        break;
                    }
                }
            }
            let _ = fs::remove_file(self.log(program));
        }

        fn log(&self, program: &str) -> PathBuf {
            self.0.join(format!("{program}.args"))
        }

        fn args(&self, program: &str) -> Option<Vec<String>> {
            let text = fs::read_to_string(self.log(program)).ok()?;
            Some(text.lines().map(str::to_string).collect())
        }

        /// Runs the walk with this directory as the children's whole `PATH`.
        ///
        /// Before any real player's name is used, a name that exists nowhere
        /// but here is walked the same way. If that is not found, the `PATH`
        /// is not in force, and the test stops there rather than find out what
        /// the real name would have run.
        fn walk(&self, candidates: &[Candidate]) -> Walk {
            const PROBE: &str = "pomodoro-stand-in-probe";
            let confined = |command: &mut Command| {
                command.env("PATH", &self.0);
            };
            self.add(PROBE, 0);
            assert_eq!(
                walk(
                    &[stand_in(PROBE, &[])],
                    Duration::from_secs(10),
                    Duration::from_millis(10),
                    confined
                ),
                Walk::Played(0),
                "the stand-in directory is not the PATH in force"
            );
            walk(
                candidates,
                Duration::from_secs(10),
                Duration::from_millis(10),
                confined,
            )
        }
    }

    impl Drop for StandIns {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_theme_player_is_run_by_name_with_the_event_id_and_nothing_follows_it() {
        let stand_ins = StandIns::new("theme");
        stand_ins.add("canberra-gtk-play", 0);
        stand_ins.add("pw-play", 0);
        let file = Path::new("/nonexistent/alarm-clock-elapsed.oga");

        let outcome = stand_ins.walk(&candidates(Cue::IntervalFinished, Some(file)));

        assert_eq!(outcome, Walk::Played(0));
        assert_eq!(
            stand_ins.args("canberra-gtk-play").unwrap(),
            [
                "-i",
                "alarm-clock-elapsed",
                "-d",
                "Interval finished",
                "--property=canberra.enable=1",
            ]
        );
        assert_eq!(stand_ins.args("pw-play"), None, "a second player was run");
    }

    #[test]
    fn a_theme_player_that_refuses_hands_over_to_the_first_file_player_installed() {
        let stand_ins = StandIns::new("fallback");
        stand_ins.add("canberra-gtk-play", 1);
        // No pw-play here: the turn passes over it to the next one.
        stand_ins.add("gst-play-1.0", 0);
        stand_ins.add("paplay", 0);
        let file = Path::new("/nonexistent/complete.oga");

        let outcome = stand_ins.walk(&candidates(Cue::TaskDone, Some(file)));

        assert_eq!(outcome, Walk::Played(2));
        assert!(stand_ins.args("canberra-gtk-play").is_some());
        assert_eq!(
            stand_ins.args("gst-play-1.0").unwrap(),
            ["--no-interactive", "-q", "/nonexistent/complete.oga"]
        );
        assert_eq!(stand_ins.args("paplay"), None);
    }
}
