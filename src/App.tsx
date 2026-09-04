import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock3, Inbox, Menu, Settings as SettingsIcon } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./lib/api";
import { defaultSnapshot } from "./types";
import type { AppSnapshot, Phase, Settings } from "./types";
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
      notificationFilter: {
        ...defaultSnapshot.settings.notificationFilter,
        enabled: true,
      },
    },
    timer: { ...defaultSnapshot.timer, activeTaskId: "preview-1" },
  };
}

async function playCompletionChime() {
  const AudioContextClass = window.AudioContext;
  if (!AudioContextClass) return;

  const context = new AudioContextClass();
  try {
    if (context.state === "suspended") {
      await context.resume();
    }
    const start = context.currentTime;
    const notes = [
      { frequency: 440, offset: 0, duration: 0.2 },
      { frequency: 554.37, offset: 0.14, duration: 0.28 },
    ];
    notes.forEach((note, index) => {
      const oscillator = context.createOscillator();
      const gain = context.createGain();
      oscillator.type = "sine";
      oscillator.frequency.value = note.frequency;
      gain.gain.setValueAtTime(0.0001, start + note.offset);
      gain.gain.exponentialRampToValueAtTime(0.035, start + note.offset + 0.018);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + note.offset + note.duration);
      oscillator.connect(gain);
      gain.connect(context.destination);
      oscillator.start(start + note.offset);
      oscillator.stop(start + note.offset + note.duration);
      if (index === notes.length - 1) {
        oscillator.onended = () => void context.close();
      }
    });
  } catch {
    void context.close();
  }
}
export default function App() {
  const inTauri = "__TAURI_INTERNALS__" in window;
  const [snapshot, setSnapshot] = useState<AppSnapshot>(
    inTauri ? defaultSnapshot : browserPreview(),
  );
  const [captureOpen, setCaptureOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [addRequest, setAddRequest] = useState(0);
  const [notice, setNotice] = useState("");
  const [ready, setReady] = useState(!inTauri);
  const noticeTimer = useRef<number | null>(null);
  const snapshotVersion = useRef(0);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const knownSessionIds = useRef(new Set<string>());
  const hasSessionBaseline = useRef(!inTauri);

  const activeTask =
    snapshot.tasks.find((task) => task.id === snapshot.timer.activeTaskId) ?? null;
  const todayFocus = snapshot.sessions.filter((session) => {
    const date = new Date(session.startedAt);
    const now = new Date();
    return (
      session.phase === "focus" &&
      session.outcome === "completed" &&
      date.toDateString() === now.toDateString()
    );
  });

  // Dialogs are modal, so remember what had focus and hand it back on close.
  // Without this, dismissing a capture with Escape drops focus to <body> and
  // the next Tab restarts from the top of the toolbar.
  const openDialog = useCallback((open: (value: boolean) => void) => {
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

  const acceptSnapshot = useCallback(
    (next: AppSnapshot, expectedVersion?: number) => {
      if (
        expectedVersion !== undefined &&
        expectedVersion !== snapshotVersion.current
      ) {
        return false;
      }
      snapshotVersion.current += 1;
      setSnapshot(next);
      return true;
    },
    [],
  );

  const run = useCallback(
    async (action: () => Promise<AppSnapshot>, successMessage?: string) => {
      if (!inTauri) {
        showNotice("Desktop controls are active in the packaged Ubuntu app.");
        return;
      }
      const requestVersion = ++snapshotVersion.current;
      try {
        const next = await action();
        acceptSnapshot(next, requestVersion);
        if (successMessage) showNotice(successMessage);
      } catch (error) {
        showNotice(typeof error === "string" ? error : "That action could not be completed.");
      }
    },
    [acceptSnapshot, inTauri, showNotice],
  );

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    const requestVersion = snapshotVersion.current;
    void api
      .snapshot()
      .then((next) => {
        if (cancelled) return;
        if (acceptSnapshot(next, requestVersion)) {
          for (const session of next.sessions) {
            knownSessionIds.current.add(session.id);
          }
          hasSessionBaseline.current = true;
        }
        setReady(true);
      })
      .catch(() => {
        if (!cancelled) {
          setReady(true);
          showNotice("Pomodoro could not open its local data.");
        }
      });

    const unlisten = listen<AppSnapshot>("state-changed", (event) => {
      if (cancelled) return;
      let hasNewCompletion = false;
      for (const session of event.payload.sessions) {
        if (knownSessionIds.current.has(session.id)) continue;
        knownSessionIds.current.add(session.id);
        if (hasSessionBaseline.current && session.outcome === "completed") {
          hasNewCompletion = true;
        }
      }
      if (hasNewCompletion && event.payload.settings.sound) {
        void playCompletionChime();
      }
      acceptSnapshot(event.payload);
    });

    return () => {
      cancelled = true;
      void unlisten.then((stop) => stop());
    };
  }, [acceptSnapshot, inTauri, showNotice]);

  // The countdown only advances while the timer is running, so that is the only
  // time the display needs repainting. Idle and paused states change solely
  // through user actions, which already arrive on the "state-changed" event —
  // polling through them was two IPC round trips per second, all day, for a
  // number that was not moving. The timer is displayed at whole-second
  // precision, so polling faster than once per second only rerenders unrelated
  // task, inbox, and ledger trees without producing a new visible value.
  const timerStatus = snapshot.timer.status;
  useEffect(() => {
    if (!inTauri || timerStatus !== "running") return;
    let cancelled = false;
    let timeout: number | null = null;
    const poll = async () => {
      const requestVersion = snapshotVersion.current;
      try {
        const next = await api.snapshot();
        if (!cancelled) acceptSnapshot(next, requestVersion);
      } catch {
        // A later state event or poll can recover from a transient IPC miss.
      } finally {
        if (!cancelled) timeout = window.setTimeout(poll, 1_000);
      }
    };
    timeout = window.setTimeout(poll, 1_000);
    return () => {
      cancelled = true;
      if (timeout !== null) window.clearTimeout(timeout);
    };
  }, [acceptSnapshot, inTauri, timerStatus]);

  useEffect(
    () => () => {
      if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    },
    [],
  );

  useEffect(() => {
    document.documentElement.dataset.theme = snapshot.settings.theme;
    const status = snapshot.timer.status === "running" ? "running" : snapshot.timer.status;
    document.title = `${Math.ceil(snapshot.timer.remainingSeconds / 60)}m · ${snapshot.timer.phase === "focus" ? "Focus" : "Break"} · ${status} — Pomodoro`;
  }, [snapshot.settings.theme, snapshot.timer]);

  const openAddTask = useCallback(() => {
    setSidebarOpen(true);
    setAddRequest((count) => count + 1);
  }, []);

  const selectPhase = useCallback(
    (phase: Phase) => void run(() => api.setPhase(phase)),
    [run],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (captureOpen) closeDialog(setCaptureOpen);
        else if (settingsOpen) closeDialog(setSettingsOpen);
        else setSidebarOpen(false);
        return;
      }
      // A modal owns the keyboard while it is open. Without this guard the
      // window-level handler still fires underneath it, so Space on a dialog
      // button would both press the button and toggle the timer behind it.
      if (captureOpen || settingsOpen) return;
      if (isTextEntry(event.target)) return;
      if (event.code === "Space") {
        // The focused control gets its own key back.
        if (activatesOnSpace(event.target)) return;
        event.preventDefault();
        void run(api.toggleTimer);
      } else if (event.ctrlKey && event.key.toLowerCase() === "i") {
        event.preventDefault();
        openDialog(setCaptureOpen);
      } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        openAddTask();
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        openDialog(setSettingsOpen);
      } else if (event.ctrlKey && ["1", "2", "3"].includes(event.key)) {
        event.preventDefault();
        const phases: Phase[] = ["focus", "shortBreak", "longBreak"];
        selectPhase(phases[Number(event.key) - 1]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [captureOpen, closeDialog, openAddTask, openDialog, run, selectPhase, settingsOpen]);

  const themeClass = useMemo(
    () => `app-shell status-${snapshot.timer.status} phase-${snapshot.timer.phase}`,
    [snapshot.timer.phase, snapshot.timer.status],
  );

  if (!ready) {
    return (
      <main className="loading-screen">
        <Clock3 aria-hidden="true" size={24} />
        <span>Opening Pomodoro…</span>
      </main>
    );
  }

  return (
    <main className={themeClass}>
      <header className="app-toolbar">
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
          {todayFocus.length} focus · {Math.round(
            todayFocus.reduce((total, item) => total + item.durationSeconds, 0) / 60,
          )}m today
        </span>
        <button
          className="icon-button"
          type="button"
          onClick={() => openDialog(setCaptureOpen)}
          aria-label="Capture an interruption"
          title="Capture interruption (Ctrl+I)"
        >
          <Inbox aria-hidden="true" size={18} />
        </button>
        <button
          className="icon-button"
          type="button"
          onClick={() => openDialog(setSettingsOpen)}
          aria-label="Open settings"
          title="Settings (Ctrl+,)"
        >
          <SettingsIcon aria-hidden="true" size={18} />
        </button>
      </header>

      <div className="workspace">
        <div className={`sidebar-wrap ${sidebarOpen ? "open" : ""}`}>
          <TaskSidebar
            tasks={snapshot.tasks}
            interruptions={snapshot.interruptions}
            notifications={snapshot.notifications}
            captureEnabled={snapshot.settings.notificationFilter.enabled}
            captureStatus={snapshot.captureStatus}
            activeTaskId={snapshot.timer.activeTaskId}
            addRequest={addRequest}
            selectionLocked={
              snapshot.timer.phase === "focus" && snapshot.timer.status !== "idle"
            }
            onSelectTask={(id) => {
              void run(() => api.selectTask(id));
              setSidebarOpen(false);
            }}
            onAddTask={async (title, estimate) => {
              await run(() => api.addTask(title, estimate), "Task added.");
            }}
            onToggleTask={(id) => void run(() => api.toggleTask(id))}
            onDeleteTask={(id) => void run(() => api.deleteTask(id), "Task deleted.")}
            onOpenCapture={() => openDialog(setCaptureOpen)}
            onHandleInterruption={(id, handled) =>
              void run(() => api.setInterruptionHandled(id, handled), "Marked handled.")
            }
            onConvertInterruption={(id) =>
              void run(() => api.convertInterruption(id), "Added to today’s tasks.")
            }
            onDeleteInterruption={(id) =>
              void run(() => api.deleteInterruption(id), "Interruption deleted.")
            }
            onTriageNotification={(id, triaged) =>
              void run(
                () => api.triageNotification(id, triaged),
                triaged ? "Marked triaged." : "Moved back to pending.",
              )
            }
            onConvertNotification={(id) =>
              void run(() => api.convertNotification(id), "Added to today’s tasks.")
            }
            onDeleteNotification={(id) =>
              void run(() => api.deleteNotification(id), "Notification deleted.")
            }
            onOpenSettings={() => openDialog(setSettingsOpen)}
          />
        </div>
        {sidebarOpen && (
          <button
            className="sidebar-scrim"
            type="button"
            aria-label="Close tasks"
            onClick={() => setSidebarOpen(false)}
          />
        )}
        <div className="focus-column">
          <TimerPanel
            timer={snapshot.timer}
            activeTask={activeTask}
            roundsBeforeLongBreak={snapshot.settings.roundsBeforeLongBreak}
            face={snapshot.settings.timerFace}
            onSetPhase={selectPhase}
            onToggleTimer={() => void run(api.toggleTimer)}
            onReset={() => void run(api.resetTimer, "Interval reset.")}
            onSkip={() =>
              void run(
                api.skipPhase,
                snapshot.timer.phase === "focus"
                  ? "Focus ended without credit."
                  : "Break skipped.",
              )
            }
            onAddTask={openAddTask}
          />
          <DayLedger
            sessions={snapshot.sessions}
            tasks={snapshot.tasks}
            interruptions={snapshot.interruptions}
          />
        </div>
      </div>

      <InterruptionDialog
        open={captureOpen}
        onClose={() => closeDialog(setCaptureOpen)}
        onSave={async (text, category) => {
          await run(
            () => api.captureInterruption(text, category),
            "Saved. Return to your focus.",
          );
        }}
      />
      <SettingsDialog
        open={settingsOpen}
        settings={snapshot.settings}
        captureStatus={snapshot.captureStatus}
        notificationCount={snapshot.notifications.length}
        onClose={() => closeDialog(setSettingsOpen)}
        onSave={async (settings: Settings) => {
          await run(() => api.updateSettings(settings), "Settings saved.");
        }}
        onClearHistory={async () => {
          await run(api.clearHistory, "Session history cleared.");
        }}
        onClearNotifications={async () => {
          await run(api.clearNotifications, "Captured notifications cleared.");
        }}
      />
      <div className="live-notice" aria-live="polite" aria-atomic="true">
        {notice}
      </div>
    </main>
  );
}
