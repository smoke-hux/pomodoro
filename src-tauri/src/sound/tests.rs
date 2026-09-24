use super::*;
use std::{
    env, fs, io,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicU32, Ordering},
    thread,
    time::Instant,
};

const NOWHERE: &str = "pomodoro-no-such-player";
fn untouched(_: &mut Command) {}

/// Every way of playing one cue's own id, in the order they are tried.
fn candidates(cue: Cue, file: Option<&Path>) -> Vec<Candidate> {
    let mut list = vec![theme_candidate(cue.event_id(), cue.description())];
    list.extend(file.into_iter().flat_map(file_candidates));
    list
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

/// The only test in the crate that runs `sleep`, which is what lets it
/// count them. The tests run in parallel, in one process: another that
/// started a `sleep` of its own would be counted here as left behind.
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
