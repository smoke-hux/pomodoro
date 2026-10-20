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

mod queue;
mod theme;

use crate::system;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};
use theme::file_finder;

/// How long one player may run. The longest sound played here is about six
/// seconds; a player still going at fifteen is not playing, it is stuck.
const PLAYER_DEADLINE: Duration = Duration::from_secs(15);
/// How often a running player is looked in on. Only the noticing of its end
/// waits on this, never the start of the sound.
const PLAYER_POLL: Duration = Duration::from_millis(100);

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

/// Plays the theme's sound for `cue`, and returns at once.
///
/// Does nothing in a test build, so that no test, present or future, can make
/// a noise by reaching this through the code it is really about. The parts
/// are tested one by one below, the spawning with stand-in programs.
pub fn play(cue: Cue) {
    if cfg!(test) {
        return;
    }
    queue::enqueue(cue, None);
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
    queue::enqueue(cue, Some(reply));
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
mod tests;
