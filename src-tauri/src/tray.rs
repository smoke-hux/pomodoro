//! System tray construction, labels, and restoring the main window.

use crate::{
    domain::{Phase, TimerStatus},
    runtime::{mutate, shut_down, toggle_timer_impl, RuntimeState},
};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Window, WindowEvent,
};

const TRAY_ID: &str = "main";

/// Invalidates a pending Linux host probe when the user reopens the window.
/// The probe never holds up GTK, and an old result cannot hide a restored
/// window or quit after a newer close request.
#[derive(Default)]
pub(crate) struct WindowCloseState {
    generation: AtomicU64,
}

impl WindowCloseState {
    fn next(&self) -> u64 {
        self.generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
    }

    #[cfg(target_os = "linux")]
    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }
}

/// On desktops without a usable tray, close exits normally so the process is
/// never left running with no visible way to reopen or quit it.
pub(crate) fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let app = window.app_handle();
    match event {
        WindowEvent::Focused(true) => {
            app.state::<WindowCloseState>().next();
        }
        WindowEvent::CloseRequested { api, .. } => {
            if app.tray_by_id(TRAY_ID).is_none() {
                return;
            }
            api.prevent_close();
            #[cfg(not(target_os = "linux"))]
            let _ = window.hide();

            #[cfg(target_os = "linux")]
            {
                let generation = app.state::<WindowCloseState>().next();
                let handle = app.clone();
                std::thread::spawn(move || {
                    let host_available = status_notifier_host_available();
                    let main_handle = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if !main_handle
                            .state::<WindowCloseState>()
                            .is_current(generation)
                        {
                            return;
                        }
                        if host_available {
                            if let Some(window) = main_handle.get_webview_window("main") {
                                let _ = window.hide();
                            }
                        } else {
                            // The same run-event shutdown path as tray Quit
                            // saves the timer and restores desktop banners.
                            main_handle.exit(0);
                        }
                    });
                });
            }
        }
        _ => {}
    }
}

/// Building an AppIndicator succeeds even if GNOME has no extension rendering
/// it. Ask the running host, rather than inferring support from the desktop
/// name or a successfully constructed tray icon. Missing tools, an absent bus,
/// and a timed-out host all conservatively mean normal close-to-exit.
#[cfg(target_os = "linux")]
fn status_notifier_host_available() -> bool {
    let mut command = crate::system::command("gdbus");
    command.args([
        "call",
        "--session",
        "--timeout",
        "1",
        "--dest",
        "org.kde.StatusNotifierWatcher",
        "--object-path",
        "/StatusNotifierWatcher",
        "--method",
        "org.freedesktop.DBus.Properties.Get",
        "org.kde.StatusNotifierWatcher",
        "IsStatusNotifierHostRegistered",
    ]);
    host_query_succeeded(crate::system::ask(command))
}

#[cfg(target_os = "linux")]
fn host_query_succeeded(answer: Option<crate::system::Answer>) -> bool {
    answer.is_some_and(|answer| answer.status.success() && answer.stdout.trim() == "(<true>,)")
}

/// The labels the tray's start/pause item can carry, indexed by
/// [`toggle_label_index`].
pub(crate) const TOGGLE_LABELS: [&str; 4] = ["Start focus", "Start break", "Pause", "Resume"];

/// What the tray's start/pause item will do if clicked now. It used to read
/// "Start / Pause" whatever the timer was doing, and the tray is exactly where
/// the window is not there to show which one applies.
pub(crate) fn toggle_label_index(status: TimerStatus, phase: Phase) -> u8 {
    match (status, phase) {
        (TimerStatus::Idle, Phase::Focus) => 0,
        (TimerStatus::Idle, Phase::ShortBreak | Phase::LongBreak) => 1,
        (TimerStatus::Running, _) => 2,
        (TimerStatus::Paused, _) => 3,
    }
}

#[cfg(test)]
fn toggle_label(status: TimerStatus, phase: Phase) -> &'static str {
    TOGGLE_LABELS[usize::from(toggle_label_index(status, phase))]
}

/// Relabels the tray item when the label needs to change.
///
/// The relabelling always runs on the main thread and takes the label from
/// the record at that moment, not from the snapshot that prompted it. Two
/// threads can ask at nearly the same time — the timer thread at a phase end,
/// the main thread on a click — and if each carried its own label across, the
/// older one could land last and stay until the next transition. Run in one
/// place and read late, whichever runs last is right.
///
/// Reading late once was still not enough, which is why the relabelling is
/// [`settle_tray`]'s loop. See there for the pause-then-resume that left
/// "Resume" on a running timer.
pub(crate) fn sync_tray(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    if state.tray_toggle.get().is_none() {
        return;
    }
    // Most publishes are a task edit or a filed notification, not a timer
    // transition; those stop here without troubling the main thread.
    if state.tray_wanted.load(Ordering::SeqCst) == state.tray_shown.load(Ordering::SeqCst) {
        return;
    }

    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<RuntimeState>();
        let Some(item) = state.tray_toggle.get() else {
            return;
        };
        settle_tray(&state.tray_wanted, &state.tray_shown, |index| {
            item.set_text(TOGGLE_LABELS[usize::from(index)]).is_ok()
        });
    });
}

/// Applies the wanted label until the label shown is the label wanted.
///
/// This used to read `wanted` once, relabel, and record what it had shown.
/// Relabelling takes time, and `shown` is only updated after it. With "Pause"
/// showing, a pause asked for "Resume" and the relabelling began; a resume
/// then asked for "Pause" again, and its publish compared that against
/// `shown` — still "Pause", the relabelling not having finished — and took
/// [`sync_tray`]'s fast path out. The relabelling then finished and recorded
/// "Resume", on a running timer, until some later publish happened to differ.
///
/// So after recording what was shown, `wanted` is read again, and a change
/// that arrived in the meantime is applied too. Only the main thread writes
/// `shown`, so the loop ends as soon as nobody is racing it. An `apply` that
/// fails ends it as well, with `shown` untouched: the next publish tries
/// again, where retrying here would spin on a tray that is not answering.
///
/// The orderings are `SeqCst` on both atomics, everywhere, because this is
/// two threads each writing one value and then reading the other. Under
/// anything weaker each may miss the other's write — the publisher sees the
/// old `shown` and leaves, the loop sees the old `wanted` and stops — which
/// is the same lost update by another road.
fn settle_tray(wanted: &AtomicU8, shown: &AtomicU8, mut apply: impl FnMut(u8) -> bool) {
    loop {
        let target = wanted.load(Ordering::SeqCst);
        if shown.load(Ordering::SeqCst) == target || !apply(target) {
            return;
        }
        shown.store(target, Ordering::SeqCst);
    }
}

pub(crate) fn show_main_window(app: &AppHandle) {
    app.state::<WindowCloseState>().next();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Pomodoro", true, None::<&str>)?;
    let state = app.state::<RuntimeState>();
    let index = state
        .data
        .lock()
        .map(|data| toggle_label_index(data.timer.status, data.timer.phase))
        .unwrap_or(0);
    state.tray_wanted.store(index, Ordering::SeqCst);
    state.tray_shown.store(index, Ordering::SeqCst);
    let label = TOGGLE_LABELS[usize::from(index)];
    let toggle = MenuItem::with_id(app, "toggle", label, true, None::<&str>)?;
    let skip = MenuItem::with_id(app, "skip", "Skip interval", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &skip, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Pomodoro focus timer")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "toggle" => {
                let state = app.state::<RuntimeState>();
                let _ = toggle_timer_impl(&state, app);
            }
            "skip" => {
                let state = app.state::<RuntimeState>();
                let _ = mutate(&state, app, |data, now| {
                    data.skip(now);
                    Ok(())
                });
            }
            "quit" => {
                shut_down(app);
                app.exit(0);
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    let _ = state.tray_toggle.set(toggle);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn restoring_the_window_cancels_a_pending_close() {
        let state = WindowCloseState::default();
        let close = state.next();
        assert!(state.is_current(close));
        state.next();
        assert!(!state.is_current(close));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_the_latest_close_request_can_hide_or_exit() {
        let state = WindowCloseState::default();
        let first = state.next();
        let second = state.next();
        assert!(!state.is_current(first));
        assert!(state.is_current(second));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_host_must_confirm_support_before_close_can_hide_the_window() {
        fn answer(script: &str) -> Option<crate::system::Answer> {
            let mut command = crate::system::command("sh");
            command.args(["-c", script]);
            crate::system::ask(command)
        }

        assert!(host_query_succeeded(answer("printf '(<true>,)\\n'")));
        assert!(!host_query_succeeded(answer("printf '(<false>,)\\n'")));
        assert!(!host_query_succeeded(answer(
            "printf '(<true>,)\\n'; exit 1"
        )));
        assert!(!host_query_succeeded(answer("printf 'true\\n'")));
        assert!(!host_query_succeeded(answer("printf ''")));
        assert!(!host_query_succeeded(None));
    }

    #[test]
    fn the_tray_item_names_what_a_click_will_do() {
        assert_eq!(toggle_label(TimerStatus::Running, Phase::Focus), "Pause");
        assert_eq!(
            toggle_label(TimerStatus::Running, Phase::ShortBreak),
            "Pause"
        );
        assert_eq!(toggle_label(TimerStatus::Paused, Phase::Focus), "Resume");
        assert_eq!(toggle_label(TimerStatus::Idle, Phase::Focus), "Start focus");
        assert_eq!(
            toggle_label(TimerStatus::Idle, Phase::ShortBreak),
            "Start break"
        );
        assert_eq!(
            toggle_label(TimerStatus::Idle, Phase::LongBreak),
            "Start break"
        );
    }

    /// The pause-then-resume race, with the resume landing while the first
    /// relabelling is still under way — after its publish has already compared
    /// against the old `shown` and left.
    #[test]
    fn a_label_wanted_during_a_relabelling_is_applied_too() {
        let wanted = AtomicU8::new(3);
        let shown = AtomicU8::new(2);
        let mut applied = Vec::new();

        settle_tray(&wanted, &shown, |index| {
            if applied.is_empty() {
                wanted.store(2, Ordering::SeqCst);
            }
            applied.push(index);
            true
        });

        assert_eq!(applied, [3, 2], "the final label is applied last");
        assert_eq!(
            shown.load(Ordering::SeqCst),
            wanted.load(Ordering::SeqCst),
            "the tray ends on what is wanted"
        );
    }

    #[test]
    fn a_tray_that_will_not_relabel_is_asked_once_and_not_recorded() {
        let wanted = AtomicU8::new(3);
        let shown = AtomicU8::new(2);
        let mut asked = 0;

        settle_tray(&wanted, &shown, |_| {
            asked += 1;
            false
        });

        assert_eq!(asked, 1);
        // Still unequal, so the next publish gets past the fast path.
        assert_eq!(shown.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_tray_already_showing_what_is_wanted_is_left_alone() {
        let wanted = AtomicU8::new(1);
        let shown = AtomicU8::new(1);
        settle_tray(&wanted, &shown, |_| panic!("nothing to apply"));
    }
}
