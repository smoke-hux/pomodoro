import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock3, Inbox, Menu, Monitor, Moon, Settings as SettingsIcon, Sun } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./lib/api";
import {
  getCompletedFocusCount,
  getCompletedFocusDisplayMinutes,
  getDayBoundsForKey,
  isWithinDay,
} from "./lib/metrics";
import {
  applyTheme,
  nextTheme,
  readRememberedTheme,
  resolveTheme,
  useSystemDark,
} from "./lib/theme";
import { useCountdown } from "./lib/useCountdown";
import { useDayKey } from "./lib/useDayKey";
import { closeRowMenus, useRowMenus } from "./lib/useRowMenus";
import { defaultSnapshot } from "./types";
import type { AppSnapshot, Phase, Settings, ThemePreference } from "./types";
import { TaskSidebar } from "./components/TaskSidebar";
import { TimerPanel } from "./components/TimerPanel";
import { DayLedger } from "./components/DayLedger";
import { InterruptionDialog } from "./components/InterruptionDialog";
import { SettingsDialog } from "./components/SettingsDialog";

function isTextEntry(target: EventTarget | null) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

/**
 * True when Space is already the focused element's own key.
 *
 * Space is the timer's shortcut, but it is also how a keyboard user presses the
 * button they have just tabbed to. The window-level handler used to swallow it
 * either way, so tabbing to "Skip" and pressing Space started the timer and left
 * the button untouched — the control looked focused and did nothing.
 */
export function activatesOnSpace(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target instanceof HTMLButtonElement || target instanceof HTMLAnchorElement) return true;
  if (target instanceof HTMLSelectElement || target instanceof HTMLTextAreaElement) return true;
  if (target instanceof HTMLInputElement) return true;
  // <summary> opens its <details> on Space; the row menus are built from them.
  if (target.tagName === "SUMMARY") return true;
  const role = target.getAttribute("role");
  return (
    role === "button" ||
    role === "checkbox" ||
    role === "radio" ||
    role === "switch" ||
    role === "tab" ||
    role === "option" ||
    role === "menuitem"
  );
}

function browserPreview(): AppSnapshot {
  const now = Date.now();
  return {
    ...defaultSnapshot,
    tasks: [
      {
        id: "preview-1",
        title: "Outline the project brief",
        estimate: 3,
        completedPomodoros: 1,
        done: false,
        createdAt: now,
        completedAt: null,
      },
      {
        id: "preview-2",
        title: "Review research notes",
        estimate: 2,
        completedPomodoros: 0,
        done: false,
        createdAt: now + 1,
        completedAt: null,
      },
    ],
    notifications: [
      {
        id: "preview-notif-1",
        appName: "Thunderbird",
        summary: "Priya Raman — Re: brief review",
        body: "Sending comments before the standup.",
        urgency: 1,
        receivedAt: now - 8 * 60_000,
        duringFocus: true,
        triaged: false,
        replacesId: 0,
        taskId: null,
      },
      {
        id: "preview-notif-2",
        appName: "Software Updater",
        summary: "Updates are available",
        body: "Security updates are ready to install.",
        urgency: 0,
        receivedAt: now - 96 * 60_000,
        duringFocus: false,
        triaged: false,
        replacesId: 0,
        taskId: null,
      },
    ],
    captureStatus: { state: "active", detail: "" },
    settings: {
      ...defaultSnapshot.settings,
      // The preview has no backend, so the remembered theme is its setting.
      theme: readRememberedTheme(),
      notificationFilter: {
        ...defaultSnapshot.settings.notificationFilter,
        enabled: true,
      },
    },
    timer: { ...defaultSnapshot.timer, activeTaskId: "preview-1" },
  };
}

const themeLabels: Record<ThemePreference, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};
const themeIcons = { system: Monitor, light: Sun, dark: Moon };

export default function App() {
  const inTauri = "__TAURI_INTERNALS__" in window;
  // A function, so the preview snapshot — and the storage read inside it — is
  // built once, not on every render and thrown away.
  const [snapshot, setSnapshot] = useState<AppSnapshot>(() =>
    inTauri ? defaultSnapshot : browserPreview(),
  );
  const [captureOpen, setCaptureOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [addRequest, setAddRequest] = useState(0);
  const [notice, setNotice] = useState("");
  const [ready, setReady] = useState(!inTauri);
  const [recoveryDismissed, setRecoveryDismissed] = useState(false);
  // Whether `snapshot` holds real settings yet, rather than the defaults.
  const [settingsLoaded, setSettingsLoaded] = useState(!inTauri);
  const noticeTimer = useRef<number | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);

  // The countdown lives here, not in the snapshot. The backend sends a
  // deadline and this window counts down to it locally; the snapshot only
  // changes when something other than the clock does.
  const remainingSeconds = useCountdown(snapshot.timer);
  const timer = useMemo(
    () => ({ ...snapshot.timer, remainingSeconds }),
    [snapshot.timer, remainingSeconds],
  );

  const activeTask = useMemo(
    () => snapshot.tasks.find((task) => task.id === snapshot.timer.activeTaskId) ?? null,
    [snapshot.tasks, snapshot.timer.activeTaskId],
  );
  // Changes at midnight, so "today" rolls over in a window left open overnight.
  const dayKey = useDayKey();
  const todaySummary = useMemo(() => {
    const day = getDayBoundsForKey(dayKey);
    const today = snapshot.sessions.filter((session) => isWithinDay(session.startedAt, day));
    return {
      count: getCompletedFocusCount(today),
      minutes: getCompletedFocusDisplayMinutes(today),
    };
  }, [snapshot.sessions, dayKey]);

  // Dialogs are modal, so remember what had focus and hand it back on close.
  // Without this, dismissing a capture with Escape drops focus to <body> and
  // the next Tab restarts from the top of the toolbar.
  const openDialog = useCallback((open: (value: boolean) => void) => {
    // A menu opened from the keyboard is still open when a shortcut opens a
    // dialog — no pointer press has happened to close it — and would sit
    // behind the modal, out of Escape's reach, still acting on its row.
    closeRowMenus();
    returnFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    open(true);
  }, []);

  const closeDialog = useCallback((open: (value: boolean) => void) => {
    open(false);
    const target = returnFocusRef.current;
    returnFocusRef.current = null;
    if (target?.isConnected) {
      requestAnimationFrame(() => target.focus());
    }
  }, []);

  const showNotice = useCallback((message: string) => {
    setNotice(message);
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(""), 3_000);
  }, []);

  // A command resolves once the backend has applied and saved it. The new state
  // arrives on "state-changed" — the single channel for every change, whether
  // it came from this window, the tray, or the clock — so there is nothing to
  // reconcile between a returned value and a broadcast one.
  const run = useCallback(
    async (action: () => Promise<void>, successMessage?: string): Promise<boolean> => {
      if (!inTauri) {
        showNotice("Desktop controls are active in the packaged Ubuntu app.");
        return false;
      }
      try {
        await action();
        if (successMessage) showNotice(successMessage);
        return true;
      } catch (error) {
        showNotice(typeof error === "string" ? error : "That action could not be completed.");
        return false;
      }
    },
    [inTauri, showNotice],
  );

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    let receivedBroadcast = false;

    const unlisten = listen<AppSnapshot>("state-changed", (event) => {
      if (cancelled) return;
      // No sound from here: the backend plays the desktop's own sound when an
      // interval ends, which works with this window hidden in the tray and
      // needs no bookkeeping about which sessions this window has seen.
      receivedBroadcast = true;
      setSnapshot(event.payload);
      setSettingsLoaded(true);
    });

    // Listening first, asking second. The other way round left a gap: a change
    // broadcast after the snapshot was taken but before the listener was
    // registered was never seen, and the window showed a finished interval as
    // still running until something else happened. And once a broadcast has
    // been applied it is the newer of the two, so a snapshot that arrives
    // after it must not replace it.
    void unlisten
      .then(
        () => undefined,
        () => undefined,
      )
      .then(() => api.snapshot())
      .then((next) => {
        if (cancelled) return;
        if (!receivedBroadcast) setSnapshot(next);
        setSettingsLoaded(true);
        setReady(true);
      })
      .catch(() => {
        if (!cancelled) {
          setReady(true);
          showNotice("Pomodoro could not open its local data.");
        }
      });

    return () => {
      cancelled = true;
      void unlisten.then((stop) => stop());
    };
  }, [inTauri, showNotice]);

  useEffect(
    () => () => {
      if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    },
    [],
  );

  // Not before the stored settings have arrived — and not at all if they never
  // do: until then the snapshot holds the default, and applying that would
  // repaint a Dark user's window as System and overwrite the remembered choice
  // that public/theme-init.js painted the first frame with.
  const systemDark = useSystemDark();
  const theme = snapshot.settings.theme;
  useEffect(() => {
    if (settingsLoaded) applyTheme(theme, systemDark);
  }, [settingsLoaded, theme, systemDark]);

  const minutesLeft = Math.ceil(remainingSeconds / 60);
  useEffect(() => {
    document.title = `${minutesLeft}m · ${timer.phase === "focus" ? "Focus" : "Break"} · ${timer.status} — Pomodoro`;
  }, [minutesLeft, timer.phase, timer.status]);

  const openAddTask = useCallback(() => {
    setSidebarOpen(true);
    setAddRequest((count) => count + 1);
  }, []);
  const openCapture = useCallback(() => openDialog(setCaptureOpen), [openDialog]);
  // Until the stored settings have arrived the snapshot holds the defaults, and
  // anything that saves settings would write those defaults over the real ones.
  const openSettings = useCallback(() => {
    if (!settingsLoaded) {
      showNotice("Settings are not available until Pomodoro has read its local data.");
      return;
    }
    openDialog(setSettingsOpen);
  }, [openDialog, settingsLoaded, showNotice]);
  const closeCapture = useCallback(() => closeDialog(setCaptureOpen), [closeDialog]);
  const closeSettings = useCallback(() => closeDialog(setSettingsOpen), [closeDialog]);

  const selectPhase = useCallback(
    (phase: Phase) => void run(() => api.setPhase(phase)),
    [run],
  );

  // Every handler below is stable across renders, which is what lets the
  // memoised sidebar, ledger and dialogs sit out the once-a-second countdown.
  const toggleTimer = useCallback(() => void run(api.toggleTimer), [run]);
  const resetTimer = useCallback(() => void run(api.resetTimer, "Interval reset."), [run]);
  const timerPhase = timer.phase;
  const skipPhase = useCallback(
    () =>
      void run(
        api.skipPhase,
        timerPhase === "focus" ? "Focus ended without credit." : "Break skipped.",
      ),
    [run, timerPhase],
  );
  const selectTask = useCallback(
    (id: string) => {
      void run(() => api.selectTask(id));
      setSidebarOpen(false);
    },
    [run],
  );
  const addTask = useCallback(
    (title: string, estimate: number) => run(() => api.addTask(title, estimate), "Task added."),
    [run],
  );
  const updateTask = useCallback(
    (id: string, title: string, estimate: number) =>
      run(() => api.updateTask(id, title, estimate), "Task updated."),
    [run],
  );
  const toggleTask = useCallback((id: string) => void run(() => api.toggleTask(id)), [run]);
  const deleteTask = useCallback(
    (id: string) => void run(() => api.deleteTask(id), "Task deleted."),
    [run],
  );
  const handleInterruption = useCallback(
    (id: string, handled: boolean) =>
      void run(
        () => api.setInterruptionHandled(id, handled),
        handled ? "Marked handled." : "Moved back to the inbox.",
      ),
    [run],
  );
  const convertInterruption = useCallback(
    (id: string) => void run(() => api.convertInterruption(id), "Added to today’s tasks."),
    [run],
  );
  const deleteInterruption = useCallback(
    (id: string) => void run(() => api.deleteInterruption(id), "Interruption deleted."),
    [run],
  );
  const triageNotification = useCallback(
    (id: string, triaged: boolean) =>
      void run(
        () => api.triageNotification(id, triaged),
        triaged ? "Marked triaged." : "Moved back to pending.",
      ),
    [run],
  );
  const convertNotification = useCallback(
    (id: string) => void run(() => api.convertNotification(id), "Added to today’s tasks."),
    [run],
  );
  const deleteNotification = useCallback(
    (id: string) => void run(() => api.deleteNotification(id), "Notification deleted."),
    [run],
  );
  const saveInterruption = useCallback(
    (text: string, category: "internal" | "external") =>
      run(() => api.captureInterruption(text, category), "Saved. Return to your focus."),
    [run],
  );
  const saveSettings = useCallback(
    (settings: Settings) => run(() => api.updateSettings(settings), "Settings saved."),
    [run],
  );
  const previewSound = useCallback(() => void run(api.previewSound), [run]);
  const clearHistory = useCallback(async () => {
    await run(api.clearHistory, "Session history cleared.");
  }, [run]);
  const clearNotifications = useCallback(async () => {
    await run(api.clearNotifications, "Captured notifications cleared.");
  }, [run]);
  const closeSidebar = useCallback(() => setSidebarOpen(false), []);

  // The toolbar button steps System → Light → Dark and saves at once. It sends
  // the settings as last received, never the settings dialog's unsaved draft.
  //
  // The saved theme only comes back with the next broadcast, so a second click
  // inside that gap steps on from the theme already asked for, not from the one
  // still on screen — otherwise two quick clicks both ask for the same thing.
  const settings = snapshot.settings;
  const requestedTheme = useRef<ThemePreference | null>(null);
  useEffect(() => {
    // Only the broadcast that carries the requested theme settles it. An
    // earlier save's broadcast arriving first must not, or a click in the
    // gap before the later one repeats a request.
    if (requestedTheme.current === settings.theme) requestedTheme.current = null;
  }, [settings.theme]);
  const cycleTheme = useCallback(() => {
    // The same guard as openSettings: this sends the whole settings object.
    if (!settingsLoaded) return;
    const next = nextTheme(requestedTheme.current ?? settings.theme);
    if (!inTauri) {
      // The browser preview has no backend to save to; the theme is the one
      // setting worth previewing anyway.
      setSnapshot((current) => ({ ...current, settings: { ...current.settings, theme: next } }));
      return;
    }
    requestedTheme.current = next;
    void run(
      () => api.updateSettings({ ...settings, theme: next }),
      `Theme: ${themeLabels[next]}.`,
    ).then(() => {
      // Saved or refused, this request is over — unless a later click has
      // replaced it. The effect above cannot be the only thing that settles
      // it: three quick clicks end on the theme they started from, the
      // broadcasts can land in one render, and nothing is seen to change.
      if (requestedTheme.current === next) requestedTheme.current = null;
    });
  }, [inTauri, run, settings, settingsLoaded]);

  useRowMenus();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // An open row menu is the innermost thing on screen, so it goes first
        // and focus returns to the button that opened it.
        const menu = document.querySelector<HTMLDetailsElement>("details.row-menu[open]");
        if (menu && !captureOpen && !settingsOpen) {
          menu.open = false;
          menu.querySelector("summary")?.focus();
          return;
        }
        if (captureOpen) closeCapture();
        else if (settingsOpen) closeSettings();
        else setSidebarOpen(false);
        return;
      }
      // A modal owns the keyboard while it is open. Without this guard the
      // window-level handler still fires underneath it, so Space on a dialog
      // button would both press the button and toggle the timer behind it.
      if (captureOpen || settingsOpen) return;
      if (isTextEntry(event.target)) return;
      if (event.code === "Space") {
        // A held key repeats, and every repeat was another start or pause —
        // dozens of saves, ending in whichever state the last one landed on.
        // With a modifier it is somebody else's shortcut, not this one.
        if (event.repeat || event.ctrlKey || event.altKey || event.metaKey) return;
        // The focused control gets its own key back.
        if (activatesOnSpace(event.target)) return;
        event.preventDefault();
        toggleTimer();
      } else if (event.ctrlKey && event.key.toLowerCase() === "i") {
        event.preventDefault();
        openCapture();
      } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        openAddTask();
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        openSettings();
      } else if (event.ctrlKey && ["1", "2", "3"].includes(event.key)) {
        event.preventDefault();
        const phases: Phase[] = ["focus", "shortBreak", "longBreak"];
        selectPhase(phases[Number(event.key) - 1]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    captureOpen,
    closeCapture,
    closeSettings,
    openAddTask,
    openCapture,
    openSettings,
    selectPhase,
    settingsOpen,
    toggleTimer,
  ]);

  // While a dialog is open everything behind it is inert: not focusable, not
  // clickable, not read out. The dialogs promise that with aria-modal, and
  // nothing but this makes it so.
  const behindModal = captureOpen || settingsOpen;
  const showRecovery = snapshot.recoveredStore !== null && !recoveryDismissed;
  const shellClass = `app-shell status-${timer.status} phase-${timer.phase}${showRecovery ? " has-banner" : ""}`;
  const ThemeIcon = themeIcons[theme];
  const themeTitle =
    theme === "system"
      ? `Theme: System (${resolveTheme(theme, systemDark)} right now)`
      : `Theme: ${themeLabels[theme]}`;
  const selectionLocked = timer.phase === "focus" && timer.status !== "idle";

  if (!ready) {
    return (
      <main className="loading-screen">
        <Clock3 aria-hidden="true" size={24} />
        <span>Opening Pomodoro…</span>
      </main>
    );
  }

  return (
    <main className={shellClass}>
      <header className="app-toolbar" inert={behindModal}>
        <button
          className="icon-button mobile-only"
          type="button"
          onClick={() => setSidebarOpen((open) => !open)}
          aria-label="Toggle tasks"
          aria-expanded={sidebarOpen}
        >
          <Menu aria-hidden="true" size={19} />
        </button>
        <div className="brand-mark" aria-hidden="true">
          <Clock3 size={18} />
        </div>
        <span className="app-name">Pomodoro</span>
        <span className="toolbar-summary">
          {todaySummary.count} focus · {todaySummary.minutes}m today
        </span>
        <button
          className="icon-button"
          type="button"
          onClick={openCapture}
          aria-label="Capture an interruption"
          title="Capture interruption (Ctrl+I)"
        >
          <Inbox aria-hidden="true" size={18} />
        </button>
        <button
          className="icon-button"
          type="button"
          onClick={cycleTheme}
          disabled={!settingsLoaded}
          aria-label={`${themeTitle}. Switch to ${themeLabels[nextTheme(theme)]}`}
          title={`${themeTitle} — click for ${themeLabels[nextTheme(theme)]}`}
        >
          <ThemeIcon aria-hidden="true" size={18} />
        </button>
        <button
          className="icon-button"
          type="button"
          onClick={openSettings}
          aria-label="Open settings"
          title="Settings (Ctrl+,)"
        >
          <SettingsIcon aria-hidden="true" size={18} />
        </button>
      </header>

      {showRecovery && (
        <div className="store-banner" role="alert">
          <p>
            <strong>Pomodoro could not read its saved data and has started fresh.</strong>{" "}
            {snapshot.recoveredStore
              ? `Nothing was deleted: the old file is kept as ${snapshot.recoveredStore}, next to pomodoro.json in the app’s data folder.`
              : "The old file could not be moved aside either, so to leave it untouched nothing is being saved: what you do now is lost when Pomodoro quits. Move or repair pomodoro.json in the app’s data folder, then start Pomodoro again."}
          </p>
          <button className="text-button" type="button" onClick={() => setRecoveryDismissed(true)}>
            Dismiss
          </button>
        </div>
      )}

      <div className="workspace" inert={behindModal}>
        <div className={`sidebar-wrap ${sidebarOpen ? "open" : ""}`}>
          <TaskSidebar
            tasks={snapshot.tasks}
            interruptions={snapshot.interruptions}
            notifications={snapshot.notifications}
            captureEnabled={snapshot.settings.notificationFilter.enabled}
            captureStatus={snapshot.captureStatus}
            activeTaskId={snapshot.timer.activeTaskId}
            addRequest={addRequest}
            selectionLocked={selectionLocked}
            onSelectTask={selectTask}
            dayKey={dayKey}
            onAddTask={addTask}
            onUpdateTask={updateTask}
            onToggleTask={toggleTask}
            onDeleteTask={deleteTask}
            onOpenCapture={openCapture}
            onHandleInterruption={handleInterruption}
            onConvertInterruption={convertInterruption}
            onDeleteInterruption={deleteInterruption}
            onTriageNotification={triageNotification}
            onConvertNotification={convertNotification}
            onDeleteNotification={deleteNotification}
            onOpenSettings={openSettings}
          />
        </div>
        {sidebarOpen && (
          <button
            className="sidebar-scrim"
            type="button"
            aria-label="Close tasks"
            onClick={closeSidebar}
          />
        )}
        <div className="focus-column">
          <TimerPanel
            timer={timer}
            activeTask={activeTask}
            roundsBeforeLongBreak={snapshot.settings.roundsBeforeLongBreak}
            face={snapshot.settings.timerFace}
            onSetPhase={selectPhase}
            onToggleTimer={toggleTimer}
            onReset={resetTimer}
            onSkip={skipPhase}
            onAddTask={openAddTask}
          />
          <DayLedger
            dayKey={dayKey}
            sessions={snapshot.sessions}
            tasks={snapshot.tasks}
            interruptions={snapshot.interruptions}
          />
        </div>
      </div>

      <InterruptionDialog open={captureOpen} onClose={closeCapture} onSave={saveInterruption} />
      <SettingsDialog
        open={settingsOpen}
        settings={snapshot.settings}
        captureStatus={snapshot.captureStatus}
        notificationCount={snapshot.notifications.length}
        onClose={closeSettings}
        onSave={saveSettings}
        onPreviewSound={previewSound}
        onClearHistory={clearHistory}
        onClearNotifications={clearNotifications}
      />
      <div className="live-notice" aria-live="polite" aria-atomic="true">
        {notice}
      </div>
    </main>
  );
}
