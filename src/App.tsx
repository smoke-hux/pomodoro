import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock3, Inbox, Menu, Monitor, Moon, Settings as SettingsIcon, Sun } from "lucide-react";
import { api } from "./lib/api";
import { useAppSnapshot } from "./lib/useAppSnapshot";
import { useNotice } from "./lib/useNotice";
import {
  getCompletedFocusCount,
  getCompletedFocusDisplayMinutes,
  getDayBoundsForKey,
  isWithinDay,
} from "./lib/metrics";
import { applyTheme, nextTheme, resolveTheme, useSystemDark } from "./lib/theme";
import { browserPreview } from "./lib/browserPreview";
import { useAppCommands } from "./lib/useAppCommands";
import { useKeyboardShortcuts } from "./lib/useKeyboardShortcuts";
import { useCountdown } from "./lib/useCountdown";
import { useDayKey } from "./lib/useDayKey";
import { closeRowMenus, useRowMenus } from "./lib/useRowMenus";
import type { ThemePreference } from "./types";
import { TaskSidebar } from "./components/TaskSidebar";
import { TimerPanel } from "./components/TimerPanel";
import { DayLedger } from "./components/DayLedger";
import { InterruptionDialog } from "./components/InterruptionDialog";
import { SettingsDialog } from "./components/SettingsDialog";

const themeLabels: Record<ThemePreference, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};
const themeIcons = { system: Monitor, light: Sun, dark: Moon };

export default function App() {
  const inTauri = "__TAURI_INTERNALS__" in window;
  const { notice, showNotice } = useNotice();
  const { snapshot, setSnapshot, ready, settingsLoaded } = useAppSnapshot(
    inTauri,
    browserPreview,
    showNotice,
  );
  const [captureOpen, setCaptureOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [addRequest, setAddRequest] = useState(0);
  const [recoveryDismissed, setRecoveryDismissed] = useState(false);
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

  const closeSidebar = useCallback(() => setSidebarOpen(false), []);
  const {
    run,
    selectPhase,
    toggleTimer,
    resetTimer,
    skipPhase,
    selectTask,
    addTask,
    updateTask,
    toggleTask,
    deleteTask,
    handleInterruption,
    convertInterruption,
    deleteInterruption,
    triageNotification,
    convertNotification,
    deleteNotification,
    saveInterruption,
    saveSettings,
    previewSound,
    clearHistory,
    clearNotifications,
  } = useAppCommands(inTauri, timer.phase, showNotice, closeSidebar);

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
  }, [inTauri, run, settings, settingsLoaded, setSnapshot]);

  useRowMenus();

  useKeyboardShortcuts({
    captureOpen,
    settingsOpen,
    closeCapture,
    closeSettings,
    closeSidebar,
    openCapture,
    openSettings,
    openAddTask,
    toggleTimer,
    selectPhase,
  });

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
              : "The original file has been left untouched, so nothing is being saved: what you do now is lost when Pomodoro quits. If the file came from a newer build, open it with that version. Otherwise, move or repair pomodoro.json in the app’s data folder, then start Pomodoro again."}
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
