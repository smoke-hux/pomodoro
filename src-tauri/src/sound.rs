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
//! file and a plain file player is tried instead: `pw-play`, `paplay`,
//! `gst-play-1.0`, in that order. Never `aplay`, which cannot decode the Ogg Vorbis
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
//! The end of an interval is the sound that must not be lost. Only one sound
//! plays at a time, and a request that arrives while another is playing used to
//! be dropped whichever it was — so ticking a task off a second before the
//! interval ran out, or pressing Test sound, silenced the alarm, and with
//! notifications off the interval then ended with nothing at all. An alarm
//! that arrives while a lesser sound is playing now waits its turn and plays
//! next; everything else arriving mid-sound is still dropped, since two chimes
//! on top of each other are worse than one.
//!
//! The ways of playing are tried in stages, and nothing is worked out before it
//! is needed. The theme player takes an id and needs no file, so the theme is
//! only searched by hand — a `gsettings` call and a walk over the disk — once
//! it has failed; doing that first delayed every alarm for a result that was
//! then thrown away. And an interval that finds no `alarm-clock-elapsed` by any
//! route borrows `complete` by every route, the theme player included: Yaru
//! ships no alarm sound, so on a machine without the `freedesktop` theme the
//! theme player refused the alarm and was never asked for the chime it had.
//!
//! [`play`] never blocks its caller, which may be the timer's reconciliation
//! thread or the main thread: everything happens on a short-lived thread of
//! its own. Children are run through [`crate::system`]: outside the AppImage's
//! environment, always reaped, and killed at a deadline — an audio server with
//! no usable sink will make a player hang.
//!
//! Every failure here is soft. A machine with no player, no theme or no audio
//! at all stays silent and says so once on stderr; the timer, the store and
//! the notifications are unaffected. Nothing is ever synthesised in place of
//! the theme's sound.

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Mutex, PoisonError},
    thread,
    time::Duration,
};

use crate::system;

const SCHEMA: &str = "org.gnome.desktop.sound";
const KEY: &str = "theme-name";

/// How long one player may run. The longest sound played here is about six
/// seconds; a player still going at fifteen is not playing, it is stuck.
const PLAYER_DEADLINE: Duration = Duration::from_secs(15);
/// How often a running player is looked in on. Only the noticing of its end
/// waits on this, never the start of the sound.
const PLAYER_POLL: Duration = Duration::from_millis(100);

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
    /// What the sound is, for the audio server's own listing of what is
    /// playing. Fixed text: nothing of the user's goes on a command line.
    fn description(self) -> &'static str {
        match self {
            Cue::IntervalFinished => "Interval finished",
            Cue::TaskDone => "Task complete",
        }
    }

    /// The ids to try, best first, from the freedesktop sound naming
    /// specification: `alarm-clock-elapsed` is defined there as "a user
    /// configured alarm elapsed", which is exactly what the end of an interval
    /// is. The cue's own comes first, and for an interval with no
    /// alarm sound anywhere, the completion chime — a short chime beats silence.
    fn event_ids(self) -> &'static [&'static str] {
        match self {
            Cue::IntervalFinished => &["alarm-clock-elapsed", "complete"],
            Cue::TaskDone => &["complete"],
        }
    }

    #[cfg(test)]
    fn event_id(self) -> &'static str {
        self.event_ids()[0]
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

/// `canberra-gtk-play` asked for an event by id: the only player that resolves
/// one in the user's theme, which is why it leads every stage.
fn theme_candidate(event_id: &str, description: &str) -> Candidate {
    Candidate::new(
        "canberra-gtk-play",
        [
            "-i",
            event_id,
            "-d",
            description,
            "--property=canberra.enable=1",
        ],
    )
}

/// The players that have to be handed a file, best first. `pw-play` is told
/// the stream is a notification so the audio server routes and ducks it as
/// one. `paplay` is not part of a default install any more, but where it is,
/// it says when it failed. `gst-play-1.0` does not: it exits with success on a
/// file it cannot find or decode and on an audio server it cannot reach, so
/// nothing after it would ever be tried, and it goes last. It reads its
/// keyboard controls from stdin unless told not to.
fn file_candidates(file: &Path) -> Vec<Candidate> {
    let file = file.as_os_str();
    vec![
        Candidate::new(
            "pw-play",
            [OsString::from("--media-role=Notification"), file.into()],
        ),
        Candidate::new("paplay", [file]),
        Candidate::new(
            "gst-play-1.0",
            [
                OsString::from("--no-interactive"),
                OsString::from("-q"),
                file.into(),
            ],
        ),
    ]
}

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
        match system::wait_until(&mut child, deadline, poll) {
            Some(status) if status.success() => return Walk::Played(index),
            Some(_) => {}
            None => return Walk::TimedOut(index),
        }
    }
    Walk::NoneWorked
}

/// How playing a cue ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Played,
    /// A player had to be killed. The sound may or may not have been heard.
    TimedOut,
    NoneWorked,
}

/// Plays `cue` by the first route that works.
///
/// For each id in turn — the cue's own, then for an interval the chime it may
/// borrow — the theme player is asked for the id, and only if that fails is
/// `find_file` asked for a file to hand the file players. `find_file` is the
/// expensive part and is not called at all on the usual path. A route that
/// runs into its deadline ends the whole attempt, for the reason [`walk`]
/// gives.
fn play_by_stages(
    cue: Cue,
    mut find_file: impl FnMut(&str) -> Option<PathBuf>,
    mut run: impl FnMut(&[Candidate]) -> Walk,
) -> Outcome {
    for event_id in cue.event_ids() {
        let theme_player = [theme_candidate(event_id, cue.description())];
        match run(&theme_player) {
            Walk::Played(_) => return Outcome::Played,
            Walk::TimedOut(_) => return Outcome::TimedOut,
            Walk::NoneWorked => {}
        }
        let Some(file) = find_file(event_id) else {
            continue;
        };
        match run(&file_candidates(&file)) {
            Walk::Played(_) => return Outcome::Played,
            Walk::TimedOut(_) => return Outcome::TimedOut,
            Walk::NoneWorked => {}
        }
    }
    Outcome::NoneWorked
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
fn file_finder() -> impl FnMut(&str) -> Option<PathBuf> {
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

/// Plays `cue` now, on this thread, and says how it went.
fn play_now(cue: Cue) -> Result<(), String> {
    let outcome = play_by_stages(cue, file_finder(), |candidates| {
        walk(
            candidates,
            PLAYER_DEADLINE,
            PLAYER_POLL,
            system::leave_the_appimage,
        )
    });
    match outcome {
        Outcome::Played => Ok(()),
        Outcome::TimedOut => Err(
            "The sound player did not finish and was stopped. The audio system may have no working output."
                .to_string(),
        ),
        Outcome::NoneWorked => Err(
            "No system sound player could play the sound. Pomodoro uses canberra-gtk-play, or pw-play, paplay or gst-play-1.0 with a sound theme installed."
                .to_string(),
        ),
    }
}

/// Somebody waiting to hear how their request went.
type Reply = mpsc::Sender<Result<(), String>>;

/// A sound asked for, and who to tell.
struct Request {
    cue: Cue,
    reply: Option<Reply>,
}

/// What is playing, and the one request allowed to wait behind it.
#[derive(Default)]
struct Slot {
    playing: Option<Cue>,
    waiting: Option<Request>,
}

/// What becomes of a new request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// Nothing is playing: play it.
    Start,
    /// It waits for the sound now playing and goes next.
    Wait,
    /// It is dropped.
    Drop,
}

/// Decides what becomes of a request for `cue`.
///
/// The end of an interval waits behind a lesser sound rather than being lost
/// to it. Everything else that arrives mid-sound is dropped — a second chime on
/// top of the first helps nobody, and an alarm already playing or already
/// waiting does not need another behind it.
fn admit(playing: Option<Cue>, something_waiting: bool, cue: Cue) -> Admission {
    match (playing, cue) {
        (None, _) => Admission::Start,
        (Some(Cue::TaskDone), Cue::IntervalFinished) if !something_waiting => Admission::Wait,
        _ => Admission::Drop,
    }
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    playing: None,
    waiting: None,
});

/// Empties the slot when the playing thread ends by any route other than the
/// ordinary one, which has already emptied it: a panic's unwinding, or a thread
/// that could not be started and whose closure is thrown away. A slot left
/// marked as playing would silence Pomodoro until it was restarted.
///
/// The ordinary route must [`release`](Occupied::release) it. The slot is free
/// the moment that route unlocks it, and a new sound may be admitted before
/// this thread is gone: emptying the slot again then would mark that sound as
/// not playing, letting a third play over it, and throw away an alarm waiting
/// behind it.
struct Occupied(&'static Mutex<Slot>);

impl Occupied {
    /// Lets go without touching the slot, for the route that has emptied it.
    fn release(self) {
        std::mem::forget(self);
    }
}

impl Drop for Occupied {
    fn drop(&mut self) {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.playing = None;
        if let Some(Request {
            reply: Some(reply), ..
        }) = slot.waiting.take()
        {
            let _ = reply.send(Err("The sound could not be played.".to_string()));
        }
    }
}

fn request(
    slot: &'static Mutex<Slot>,
    cue: Cue,
    reply: Option<Reply>,
    play: fn(Cue) -> Result<(), String>,
) {
    let refused = |reply: Option<Reply>| {
        if let Some(reply) = reply {
            let _ = reply.send(Err(
                "Another sound is playing. Try again in a moment.".to_string()
            ));
        }
    };
    {
        let mut held = slot.lock().unwrap_or_else(PoisonError::into_inner);
        match admit(held.playing, held.waiting.is_some(), cue) {
            Admission::Start => held.playing = Some(cue),
            Admission::Wait => {
                held.waiting = Some(Request { cue, reply });
                return;
            }
            Admission::Drop => {
                drop(held);
                refused(reply);
                return;
            }
        }
    }

    let occupied = Occupied(slot);
    let spawned = thread::Builder::new()
        .name("sound-player".to_string())
        .spawn(move || {
            let occupied = occupied;
            let mut current = Request { cue, reply };
            loop {
                let result = play(current.cue);
                if let Err(error) = &result {
                    eprintln!("could not play a sound: {error}");
                }
                if let Some(reply) = current.reply.take() {
                    let _ = reply.send(result);
                }
                let mut held = occupied.0.lock().unwrap_or_else(PoisonError::into_inner);
                match held.waiting.take() {
                    Some(next) => {
                        held.playing = Some(next.cue);
                        current = next;
                    }
                    None => {
                        held.playing = None;
                        drop(held);
                        occupied.release();
                        break;
                    }
                }
            }
        });
    if let Err(error) = spawned {
        // The closure, and the `Occupied` inside it, has been dropped: the slot
        // is free again and anything that was waiting has been told.
        eprintln!("could not play a sound: {error}");
    }
}

/// Plays the theme's sound for `cue`, and returns at once.
///
/// Does nothing in a test build, so that no test, present or future, can make
/// a noise by reaching this through the code it is really about. The parts
/// are tested one by one below, the spawning with stand-in programs.
pub fn play(cue: Cue) {
    if cfg!(test) {
        return;
    }
    request(&SLOT, cue, None, play_now);
}

/// Plays the theme's sound for `cue` and waits to say how it went, for the
/// "Test sound" button: the one caller whose whole question is whether anything
/// will be heard. Blocks until the sound has finished, so never call it from
/// the main thread or the timer's.
pub fn play_and_report(cue: Cue) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    let (reply, outcome) = mpsc::channel();
    request(&SLOT, cue, Some(reply), play_now);
    outcome
        .recv()
        .unwrap_or_else(|_| Err("The sound could not be played.".to_string()))
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
        time::Instant,
    };

    const NOWHERE: &str = "pomodoro-no-such-player";
    const SYSTEM_SOUNDS: &str = "/usr/share/sounds";

    fn system_sounds() -> Vec<PathBuf> {
        vec![PathBuf::from(SYSTEM_SOUNDS)]
    }

    fn untouched(_: &mut Command) {}

    /// Every way of playing one cue's own id, in the order they are tried.
    fn candidates(cue: Cue, file: Option<&Path>) -> Vec<Candidate> {
        let mut list = vec![theme_candidate(cue.event_id(), cue.description())];
        list.extend(file.into_iter().flat_map(file_candidates));
        list
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
                vec!["paplay", file],
                vec!["gst-play-1.0", "--no-interactive", "-q", file],
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

    /// Runs the stages against a script of answers, recording what was asked.
    fn staged(cue: Cue, files: &[(&str, &str)], answers: &[Walk]) -> (Outcome, Vec<String>) {
        let asked = std::cell::RefCell::new(Vec::new());
        let mut answers = answers.iter();
        let outcome = play_by_stages(
            cue,
            |event_id| {
                asked.borrow_mut().push(format!("find {event_id}"));
                files
                    .iter()
                    .find(|(id, _)| *id == event_id)
                    .map(|(_, path)| PathBuf::from(path))
            },
            |candidates| {
                asked.borrow_mut().push(format!(
                    "run {} {}",
                    candidates[0].program,
                    candidates[0].args.last().unwrap().to_string_lossy()
                ));
                *answers.next().expect("more stages were run than expected")
            },
        );
        (outcome, asked.into_inner())
    }

    #[test]
    fn the_theme_is_not_searched_by_hand_when_the_theme_player_works() {
        // The search is a gsettings call and a walk over the disk. Doing it
        // first delayed every alarm for a result the theme player never uses.
        let (outcome, asked) = staged(Cue::IntervalFinished, &[], &[Walk::Played(0)]);
        assert_eq!(outcome, Outcome::Played);
        assert_eq!(
            asked,
            ["run canberra-gtk-play --property=canberra.enable=1"]
        );
    }

    #[test]
    fn an_interval_with_no_alarm_sound_asks_the_theme_player_for_the_chime_too() {
        // Yaru ships no alarm sound. Without the freedesktop theme the theme
        // player refuses the alarm, no file exists for the file players, and
        // the interval used to end in silence though the theme player could
        // have played the chime.
        let (outcome, asked) = staged(
            Cue::IntervalFinished,
            &[],
            &[Walk::NoneWorked, Walk::Played(0)],
        );
        assert_eq!(outcome, Outcome::Played);
        assert_eq!(
            asked,
            [
                "run canberra-gtk-play --property=canberra.enable=1",
                "find alarm-clock-elapsed",
                "run canberra-gtk-play --property=canberra.enable=1",
            ]
        );
    }

    #[test]
    fn the_alarm_by_any_route_comes_before_the_chime_by_any_route() {
        let alarm = "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga";
        let chime = "/usr/share/sounds/Yaru/stereo/complete.oga";
        let (outcome, asked) = staged(
            Cue::IntervalFinished,
            &[("alarm-clock-elapsed", alarm), ("complete", chime)],
            &[Walk::NoneWorked; 4],
        );
        assert_eq!(outcome, Outcome::NoneWorked);
        assert_eq!(
            asked,
            [
                "run canberra-gtk-play --property=canberra.enable=1".to_string(),
                "find alarm-clock-elapsed".to_string(),
                format!("run pw-play {alarm}"),
                "run canberra-gtk-play --property=canberra.enable=1".to_string(),
                "find complete".to_string(),
                format!("run pw-play {chime}"),
            ]
        );

        // A finished task has no second sound to borrow.
        let (outcome, asked) = staged(Cue::TaskDone, &[], &[Walk::NoneWorked]);
        assert_eq!(outcome, Outcome::NoneWorked);
        assert_eq!(
            asked.len(),
            2,
            "its own id by both routes, and nothing more"
        );
    }

    #[test]
    fn a_route_that_hangs_ends_the_whole_attempt() {
        let (outcome, asked) = staged(Cue::IntervalFinished, &[], &[Walk::TimedOut(0)]);
        assert_eq!(outcome, Outcome::TimedOut);
        assert_eq!(asked.len(), 1, "the sound may already have been heard");
    }

    #[test]
    fn the_alarm_waits_behind_a_lesser_sound_and_everything_else_mid_sound_is_dropped() {
        use Admission::{Drop, Start, Wait};
        use Cue::{IntervalFinished as Alarm, TaskDone as Chime};

        assert_eq!(admit(None, false, Alarm), Start);
        assert_eq!(admit(None, false, Chime), Start);
        // Ticking a task off a second before the interval ends used to cost
        // the alarm; with notifications off the interval then ended unmarked.
        assert_eq!(admit(Some(Chime), false, Alarm), Wait);
        assert_eq!(
            admit(Some(Chime), true, Alarm),
            Drop,
            "one alarm waiting is enough"
        );
        assert_eq!(admit(Some(Alarm), false, Alarm), Drop);
        assert_eq!(admit(Some(Alarm), false, Chime), Drop);
        assert_eq!(admit(Some(Chime), false, Chime), Drop);
    }

    /// What the stand-in `play` functions below were asked to play.
    static PLAYED: Mutex<Vec<Cue>> = Mutex::new(Vec::new());

    #[test]
    fn an_alarm_asked_for_mid_chime_is_played_next_and_its_asker_is_told() {
        static SLOT: Mutex<Slot> = Mutex::new(Slot {
            playing: None,
            waiting: None,
        });
        fn slow(cue: Cue) -> Result<(), String> {
            PLAYED.lock().unwrap().push(cue);
            thread::sleep(Duration::from_millis(150));
            Ok(())
        }

        request(&SLOT, Cue::TaskDone, None, slow);
        let (reply, outcome) = mpsc::channel();
        request(&SLOT, Cue::IntervalFinished, Some(reply), slow);
        // A chime arriving now is dropped, and says so at once.
        let (dropped, refusal) = mpsc::channel();
        request(&SLOT, Cue::TaskDone, Some(dropped), slow);
        assert!(refusal
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_err());

        assert_eq!(
            outcome.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );
        assert_eq!(
            *PLAYED.lock().unwrap(),
            [Cue::TaskDone, Cue::IntervalFinished]
        );

        // And the slot is free again afterwards.
        for _ in 0..50 {
            if SLOT.lock().unwrap().playing.is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(SLOT.lock().unwrap().playing.is_none());
    }

    #[test]
    fn a_sound_that_fails_is_reported_to_whoever_asked() {
        static SLOT: Mutex<Slot> = Mutex::new(Slot {
            playing: None,
            waiting: None,
        });
        fn broken(_: Cue) -> Result<(), String> {
            Err("no player".to_string())
        }
        let (reply, outcome) = mpsc::channel();
        request(&SLOT, Cue::IntervalFinished, Some(reply), broken);
        assert_eq!(
            outcome.recv_timeout(Duration::from_secs(5)).unwrap(),
            Err("no player".to_string())
        );
    }

    #[test]
    fn the_slot_is_given_back_when_the_playing_thread_panics_or_never_starts() {
        static SLOT: Mutex<Slot> = Mutex::new(Slot {
            playing: None,
            waiting: None,
        });
        let (reply, outcome) = mpsc::channel();
        {
            let mut slot = SLOT.lock().unwrap();
            slot.playing = Some(Cue::TaskDone);
            slot.waiting = Some(Request {
                cue: Cue::IntervalFinished,
                reply: Some(reply),
            });
        }
        let occupied = Occupied(&SLOT);
        let unwound = thread::spawn(move || {
            let _occupied = occupied;
            panic!("a panic on the playing thread, on purpose");
        })
        .join();
        assert!(unwound.is_err());

        let slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(
            slot.playing.is_none(),
            "the slot was left marked as playing"
        );
        assert!(slot.waiting.is_none());
        assert!(
            outcome.recv().unwrap().is_err(),
            "whoever was waiting is told"
        );
    }

    #[test]
    fn a_released_slot_is_not_emptied_again_behind_the_next_sound() {
        static SLOT: Mutex<Slot> = Mutex::new(Slot {
            playing: None,
            waiting: None,
        });
        let occupied = Occupied(&SLOT);
        // The ordinary route has emptied the slot and unlocked it, and a new
        // sound, with an alarm waiting behind it, has already been admitted.
        let (reply, outcome) = mpsc::channel();
        {
            let mut slot = SLOT.lock().unwrap();
            slot.playing = Some(Cue::TaskDone);
            slot.waiting = Some(Request {
                cue: Cue::IntervalFinished,
                reply: Some(reply),
            });
        }
        occupied.release();

        let slot = SLOT.lock().unwrap();
        assert_eq!(slot.playing, Some(Cue::TaskDone));
        assert_eq!(
            slot.waiting.as_ref().map(|next| next.cue),
            Some(Cue::IntervalFinished)
        );
        assert!(outcome.try_recv().is_err(), "the waiting alarm was dropped");
    }

    #[test]
    fn sounds_asked_for_from_many_threads_never_play_over_one_another() {
        static SLOT: Mutex<Slot> = Mutex::new(Slot {
            playing: None,
            waiting: None,
        });
        static AT_ONCE: AtomicU32 = AtomicU32::new(0);
        static MOST: AtomicU32 = AtomicU32::new(0);
        fn counted(_: Cue) -> Result<(), String> {
            let now = AT_ONCE.fetch_add(1, Ordering::SeqCst) + 1;
            MOST.fetch_max(now, Ordering::SeqCst);
            AT_ONCE.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
        let askers: Vec<_> = (0..4)
            .map(|_| {
                thread::spawn(|| {
                    for _ in 0..20_000 {
                        request(&SLOT, Cue::TaskDone, None, counted);
                    }
                })
            })
            .collect();
        for asker in askers {
            asker.join().unwrap();
        }
        for _ in 0..100 {
            if SLOT.lock().unwrap().playing.is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(MOST.load(Ordering::SeqCst), 1, "two sounds played at once");
    }

    #[test]
    fn playing_does_nothing_under_test() {
        play(Cue::IntervalFinished);
        play(Cue::TaskDone);
        assert_eq!(play_and_report(Cue::IntervalFinished), Ok(()));
        assert!(SLOT.lock().unwrap().playing.is_none());
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
        stand_ins.add("paplay", 0);
        stand_ins.add("gst-play-1.0", 0);
        let file = Path::new("/nonexistent/complete.oga");

        let outcome = stand_ins.walk(&candidates(Cue::TaskDone, Some(file)));

        assert_eq!(outcome, Walk::Played(2));
        assert!(stand_ins.args("canberra-gtk-play").is_some());
        assert_eq!(
            stand_ins.args("paplay").unwrap(),
            ["/nonexistent/complete.oga"]
        );
        assert_eq!(stand_ins.args("gst-play-1.0"), None);
    }

    #[test]
    fn a_file_player_that_fails_hands_over_to_the_one_that_cannot_say_so() {
        // gst-play-1.0 exits with success whether or not it played, so a
        // player that does report failure has to have had its turn first.
        let stand_ins = StandIns::new("unreliable");
        stand_ins.add("canberra-gtk-play", 1);
        stand_ins.add("pw-play", 1);
        stand_ins.add("paplay", 1);
        stand_ins.add("gst-play-1.0", 0);
        let file = Path::new("/nonexistent/complete.oga");

        let outcome = stand_ins.walk(&candidates(Cue::TaskDone, Some(file)));

        assert_eq!(outcome, Walk::Played(3));
        assert!(stand_ins.args("pw-play").is_some());
        assert!(stand_ins.args("paplay").is_some());
    }
}
