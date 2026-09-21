//! Running the desktop's own command-line tools — `gsettings`, the sound
//! players — as children of Pomodoro.
//!
//! Two things go wrong when that is done naively, and both are the same for
//! every tool, so they are dealt with once, here.
//!
//! **The AppImage's environment.** An AppImage runs with variables pointing at
//! the libraries, GTK modules and GSettings schemas bundled inside it. A system
//! binary that inherits them loads the wrong libraries or looks in the wrong
//! schema directory and fails: a sound player dies before it plays, and
//! `gsettings` cannot find the desktop's keys — which silently disabled
//! "silence banners during focus" in that build, not just sound. Children are
//! therefore handed an environment with the bundle taken back out of it.
//!
//! Taken out precisely: an entry is dropped only if it points inside the
//! bundle (`$APPDIR`). Removing a variable such as `LD_LIBRARY_PATH` outright
//! would also throw away what the user had put there themselves — a PipeWire
//! built into their home directory, say — and break the player for the opposite
//! reason.
//!
//! **Waiting.** A child that never exits would hold up whoever waits for it,
//! and some of these calls are made with the application's data locked. Every
//! wait here has a deadline, after which the child is killed — and always
//! reaped, because this is a tray application that runs for days and an
//! unreaped child is a zombie for all of them.

use std::{
    env,
    ffi::{OsStr, OsString},
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

/// How long a query such as `gsettings get` is given. It answers in
/// milliseconds when it answers at all.
pub const QUERY_DEADLINE: Duration = Duration::from_secs(2);
const QUERY_POLL: Duration = Duration::from_millis(10);

/// What the environment of a child should be, given the parent's and the
/// directory the AppImage is mounted at: for each variable that needs
/// changing, its new value, or `None` to remove it.
///
/// Every variable is treated as a `:`-separated list, which is what the ones
/// that matter are, and a single path is a list of one. Entries inside
/// `appdir` go; the rest stay in order; a variable left with nothing is
/// removed. A variable that never mentioned the bundle is not touched at all.
fn without_the_bundle(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    appdir: &Path,
) -> Vec<(OsString, Option<OsString>)> {
    let mut changes = Vec::new();
    for (name, value) in vars {
        let Some(text) = value.to_str() else {
            continue;
        };
        let kept: Vec<&str> = text
            .split(':')
            .filter(|entry| !Path::new(entry).starts_with(appdir))
            .collect();
        if kept.len() == text.split(':').count() {
            continue;
        }
        let kept = kept.join(":");
        changes.push((name, (!kept.is_empty()).then(|| OsString::from(kept))));
    }
    changes
}

/// Takes a child about to be spawned out of the AppImage it was launched from,
/// if there is one. Outside an AppImage this does nothing.
pub fn leave_the_appimage(command: &mut Command) {
    let Some(appdir) = env::var_os("APPDIR").filter(|_| env::var_os("APPIMAGE").is_some()) else {
        return;
    };
    for (name, value) in without_the_bundle(env::vars_os(), Path::new(&appdir)) {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
}

/// A command for one of the desktop's tools, found through `PATH`, with the
/// AppImage taken out of its environment and no terminal to read from.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    leave_the_appimage(&mut command);
    command
}

/// Waits for `child` until `deadline` has passed, and reaps it either way.
///
/// `None` means it had to be killed, and so nothing is known about what it
/// did. A `try_wait` that fails is treated the same: whatever became of the
/// child, the one thing still owed is that it does not outlive this call as a
/// zombie.
pub fn wait_until(child: &mut Child, deadline: Duration, poll: Duration) -> Option<ExitStatus> {
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

/// What a finished query printed.
pub struct Answer {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a short query and returns what it printed, or `None` if it could not
/// be started or did not finish inside [`QUERY_DEADLINE`].
///
/// Only for tools whose whole output is far smaller than a pipe's buffer, so
/// that the child can finish writing, and exit, before anything is read.
pub fn ask(mut command: Command) -> Option<Answer> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().ok()?;
    let status = wait_until(&mut child, QUERY_DEADLINE, QUERY_POLL)?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    child.stderr.take()?.read_to_string(&mut stderr).ok()?;
    Some(Answer {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn change<'a>(
        changes: &'a [(OsString, Option<OsString>)],
        name: &str,
    ) -> Option<&'a Option<OsString>> {
        changes
            .iter()
            .find(|(changed, _)| changed == name)
            .map(|(_, value)| value)
    }

    #[test]
    fn only_what_points_into_the_bundle_is_taken_out() {
        let appdir = Path::new("/tmp/.mount_PomoXYZ");
        let changes = without_the_bundle(
            vars(&[
                // The user's own entry has to survive: removing the variable
                // outright broke a player built against a private PipeWire.
                (
                    "LD_LIBRARY_PATH",
                    "/tmp/.mount_PomoXYZ/usr/lib:/home/me/pipewire/lib",
                ),
                (
                    "GSETTINGS_SCHEMA_DIR",
                    "/tmp/.mount_PomoXYZ/usr/share/glib-2.0/schemas",
                ),
                (
                    "PATH",
                    "/tmp/.mount_PomoXYZ/usr/bin:/usr/local/bin:/usr/bin",
                ),
                ("HOME", "/home/me"),
                ("GDK_BACKEND", "x11"),
            ]),
            appdir,
        );

        assert_eq!(
            change(&changes, "LD_LIBRARY_PATH"),
            Some(&Some(OsString::from("/home/me/pipewire/lib")))
        );
        assert_eq!(change(&changes, "GSETTINGS_SCHEMA_DIR"), Some(&None));
        assert_eq!(
            change(&changes, "PATH"),
            Some(&Some(OsString::from("/usr/local/bin:/usr/bin")))
        );
        assert_eq!(change(&changes, "HOME"), None, "untouched, not rewritten");
        assert_eq!(change(&changes, "GDK_BACKEND"), None);
    }

    #[test]
    fn a_directory_that_merely_starts_with_the_same_letters_is_not_the_bundle() {
        let changes = without_the_bundle(
            vars(&[(
                "XDG_DATA_DIRS",
                "/tmp/.mount_PomoXYZ-other/share:/usr/share",
            )]),
            Path::new("/tmp/.mount_PomoXYZ"),
        );
        assert!(changes.is_empty());
    }

    #[test]
    fn a_query_is_answered_with_what_it_printed() {
        let mut command = command("sh");
        command.args(["-c", "echo out; echo err >&2; exit 3"]);
        let answer = ask(command).expect("sh runs and exits");
        assert_eq!(answer.status.code(), Some(3));
        assert_eq!(answer.stdout.trim(), "out");
        assert_eq!(answer.stderr.trim(), "err");
    }

    #[test]
    fn a_tool_that_is_not_installed_is_no_answer() {
        assert!(ask(command("pomodoro-no-such-tool")).is_none());
    }

    #[test]
    fn a_child_that_will_not_finish_is_killed_and_reaped() {
        // Not `sleep`: the sound tests count this process's `sleep` children
        // to prove their own was reaped, and run alongside this one.
        let mut child = command("tail").args(["-f", "/dev/null"]).spawn().unwrap();
        let began = Instant::now();
        let status = wait_until(
            &mut child,
            Duration::from_millis(200),
            Duration::from_millis(20),
        );
        assert!(status.is_none());
        assert!(began.elapsed() < Duration::from_secs(5));
        // Reaped: a second wait has nothing left to collect but still answers.
        assert!(child.try_wait().is_ok());
    }
}
