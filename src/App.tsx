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
  const [notice, setNotice] = useState("");
  const [ready, setReady] = useState(!inTauri);
  const noticeTimer = useRef<number | null>(null);
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

  const showNotice = useCallback((message: string) => {
    setNotice(message);
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(""), 3_000);
  }, []);

  const run = useCallback(
    async (action: () => Promise<AppSnapshot>, successMessage?: string) => {
      if (!inTauri) {
        showNotice("Desktop controls are active in the packaged Ubuntu app.");
        return;
      }
      try {
        const next = await action();
        setSnapshot(next);
        if (successMessage) showNotice(successMessage);
      } catch (error) {
        showNotice(typeof error === "string" ? error : "That action could not be completed.");
      }
    },
    [inTauri, showNotice],
  );

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    void api
      .snapshot()
      .then((next) => {
        if (!cancelled) {
          for (const session of next.sessions) {
            knownSessionIds.current.add(session.id);
          }
          hasSessionBaseline.current = true;
          setSnapshot(next);
          setReady(true);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setReady(true);
          showNotice("Pomodoro could not open its local data.");
        }
      });

    const poll = window.setInterval(() => {
      void api.snapshot().then((next) => !cancelled && setSnapshot(next)).catch(() => undefined);
    }, 500);

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
      setSnapshot(event.payload);
    });

    return () => {
      cancelled = true;
      window.clearInterval(poll);
      void unlisten.then((stop) => stop());
    };
  }, [inTauri, showNotice]);

  useEffect(() => {
    document.documentElement.dataset.theme = snapshot.settings.theme;
    const status = snapshot.timer.status === "running" ? "running" : snapshot.timer.status;
    document.title = `${Math.ceil(snapshot.timer.remainingSeconds / 60)}m · ${snapshot.timer.phase === "focus" ? "Focus" : "Break"} · ${status} — Pomodoro`;
  }, [snapshot.settings.theme, snapshot.timer]);

  const openAddTask = useCallback(() => {
    setSidebarOpen(true);
    window.setTimeout(() => {
      const addButton = document.querySelector<HTMLButtonElement>(
        'button[aria-label="Add a task"]',
      );
      addButton?.click();
    }, 0);
  }, []);

  const selectPhase = useCallback(
    (phase: Phase) => void run(() => api.setPhase(phase)),
    [run],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setCaptureOpen(false);
        setSettingsOpen(false);
        setSidebarOpen(false);
        return;
      }
      if (isTextEntry(event.target)) return;
      if (event.code === "Space") {
        event.preventDefault();
        void run(api.toggleTimer);
      } else if (event.ctrlKey && event.key.toLowerCase() === "i") {
        event.preventDefault();
        setCaptureOpen(true);
      } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        openAddTask();
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        setSettingsOpen(true);
      } else if (event.ctrlKey && ["1", "2", "3"].includes(event.key)) {
        event.preventDefault();
        const phases: Phase[] = ["focus", "shortBreak", "longBreak"];
        selectPhase(phases[Number(event.key) - 1]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openAddTask, run, selectPhase]);

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
          onClick={() => setCaptureOpen(true)}
          aria-label="Capture an interruption"
          title="Capture interruption (Ctrl+I)"
        >
          <Inbox aria-hidden="true" size={18} />
        </button>
        <button
          className="icon-button"
          type="button"
          onClick={() => setSettingsOpen(true)}
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
            activeTaskId={snapshot.timer.activeTaskId}
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
            onOpenCapture={() => setCaptureOpen(true)}
            onHandleInterruption={(id, handled) =>
              void run(() => api.setInterruptionHandled(id, handled), "Marked handled.")
            }
            onConvertInterruption={(id) =>
              void run(() => api.convertInterruption(id), "Added to today’s tasks.")
            }
            onDeleteInterruption={(id) =>
              void run(() => api.deleteInterruption(id), "Interruption deleted.")
            }
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
        onClose={() => setCaptureOpen(false)}
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
        onClose={() => setSettingsOpen(false)}
        onSave={async (settings: Settings) => {
          await run(() => api.updateSettings(settings), "Settings saved.");
        }}
        onClearHistory={async () => {
          await run(api.clearHistory, "Session history cleared.");
        }}
      />
      <div className="live-notice" aria-live="polite" aria-atomic="true">
        {notice}
      </div>
    </main>
  );
}
