import { useEffect, useState } from "react";
import { Pause, Play, RotateCcw, SkipForward } from "lucide-react";
import type { FocusTask, Phase, TimerState } from "../types";

const phaseLabels: Record<Phase, string> = {
  focus: "Focus",
  shortBreak: "Short break",
  longBreak: "Long break",
};

interface TimerPanelProps {
  timer: TimerState;
  activeTask: FocusTask | null;
  roundsBeforeLongBreak: number;
  onSetPhase: (phase: Phase) => void;
  onToggleTimer: () => void;
  onReset: () => void;
  onSkip: () => void;
  onAddTask: () => void;
}

function countdown(seconds: number) {
  const safe = Math.max(0, Math.ceil(seconds));
  return `${Math.floor(safe / 60).toString().padStart(2, "0")}:${(safe % 60)
    .toString()
    .padStart(2, "0")}`;
}

export function TimerPanel({
  timer,
  activeTask,
  roundsBeforeLongBreak,
  onSetPhase,
  onToggleTimer,
  onReset,
  onSkip,
  onAddTask,
}: TimerPanelProps) {
  const progress =
    timer.durationSeconds > 0
      ? ((timer.durationSeconds - timer.remainingSeconds) / timer.durationSeconds) * 100
      : 0;
  const needsTask = timer.phase === "focus" && !activeTask;

  // Skipping a break costs nothing. Skipping a focus interval throws away the
  // elapsed work — and the control sits next to Reset with the same styling, so
  // it asks once first. Breaks stay a single click.
  const [confirmSkip, setConfirmSkip] = useState(false);
  const skipDiscardsCredit = timer.phase === "focus" && timer.status !== "idle";

  useEffect(() => {
    setConfirmSkip(false);
  }, [timer.phase, timer.status]);

  useEffect(() => {
    if (!confirmSkip) return;
    const timeout = window.setTimeout(() => setConfirmSkip(false), 4_000);
    return () => window.clearTimeout(timeout);
  }, [confirmSkip]);

  const handleSkip = () => {
    if (skipDiscardsCredit && !confirmSkip) {
      setConfirmSkip(true);
      return;
    }
    setConfirmSkip(false);
    onSkip();
  };

  const actionLabel =
    timer.status === "running"
      ? "Pause"
      : timer.status === "paused"
        ? "Resume"
        : timer.phase === "focus"
          ? "Start focus"
          : "Start break";

  return (
    <section className={`timer-panel phase-${timer.phase}`} aria-labelledby="timer-heading">
      <div className="phase-tabs" role="tablist" aria-label="Timer mode">
        {(Object.keys(phaseLabels) as Phase[]).map((phase) => (
          <button
            key={phase}
            type="button"
            role="tab"
            aria-selected={timer.phase === phase}
            disabled={timer.status !== "idle"}
            onClick={() => onSetPhase(phase)}
          >
            {phaseLabels[phase]}
          </button>
        ))}
      </div>

      <div className="cycle-count" aria-label={`Round ${timer.completedInCycle + 1} of ${roundsBeforeLongBreak}`}>
        <span>
          {timer.phase === "focus"
            ? `Round ${Math.min(timer.completedInCycle + 1, roundsBeforeLongBreak)} of ${roundsBeforeLongBreak}`
            : timer.phase === "longBreak"
              ? "Cycle complete"
              : `${timer.completedInCycle} of ${roundsBeforeLongBreak} rounds`}
        </span>
        <span className="round-marks" aria-hidden="true">
          {Array.from({ length: roundsBeforeLongBreak }, (_, index) => (
            <i key={index} className={index < timer.completedInCycle ? "filled" : ""} />
          ))}
        </span>
      </div>

      <div className="timer-stage">
        <h1 id="timer-heading" className="visually-hidden">
          {phaseLabels[timer.phase]} timer
        </h1>
        <output
          className="timer-digits"
          aria-label={`${Math.ceil(timer.remainingSeconds / 60)} minutes remaining`}
        >
          {countdown(timer.remainingSeconds)}
        </output>
        <div
          className="progress-track"
          role="progressbar"
          aria-label={`${phaseLabels[timer.phase]} progress`}
          aria-valuemin={0}
          aria-valuemax={timer.durationSeconds}
          aria-valuenow={Math.round(timer.durationSeconds - timer.remainingSeconds)}
        >
          <span style={{ width: `${Math.min(100, Math.max(0, progress))}%` }} />
        </div>

        <div className="session-task">
          {timer.phase === "focus" ? (
            activeTask ? (
              <>
                <strong>{activeTask.title}</strong>
                <span>
                  session {activeTask.completedPomodoros + 1} of {activeTask.estimate}
                </span>
              </>
            ) : (
              <>
                <strong>No task selected</strong>
                <button type="button" className="inline-action" onClick={onAddTask}>
                  Add a task to begin
                </button>
              </>
            )
          ) : (
            <>
              <strong>{timer.phase === "longBreak" ? "Take a proper break" : "Step away for a moment"}</strong>
              <span>Look away from the screen, move, and reset.</span>
            </>
          )}
        </div>

        <div className="timer-controls">
          <button
            className="primary-control"
            type="button"
            onClick={onToggleTimer}
            disabled={needsTask}
          >
            {timer.status === "running" ? (
              <Pause aria-hidden="true" size={18} />
            ) : (
              <Play aria-hidden="true" size={18} fill="currentColor" />
            )}
            {actionLabel}
          </button>
          <button
            className="secondary-control"
            type="button"
            onClick={onReset}
            disabled={timer.status === "idle" && timer.remainingSeconds === timer.durationSeconds}
            title="Reset this interval"
          >
            <RotateCcw aria-hidden="true" size={17} /> Reset
          </button>
          <button
            className={`secondary-control${confirmSkip ? " confirming" : ""}`}
            type="button"
            onClick={handleSkip}
            title={timer.phase === "focus" ? "End focus without credit" : "Skip this break"}
          >
            <SkipForward aria-hidden="true" size={17} />{" "}
            {confirmSkip ? "Discard session?" : "Skip"}
          </button>
        </div>
        <p className="shortcut-hint">
          <kbd>Space</kbd> {timer.status === "running" ? "pause" : "start"}
          <span aria-hidden="true">·</span>
          <kbd>Ctrl I</kbd> capture a distraction
        </p>
      </div>
    </section>
  );
}
