import { memo } from "react";
import { ChevronDown } from "lucide-react";
import { getCompletedFocusMinutes, getLocalDateKey } from "../lib/metrics";
import type { FocusTask, Interruption, SessionRecord } from "../types";

interface DayLedgerProps {
  /** Today's local date. "Today" is whatever falls on it, so the ledger rolls over when it changes at midnight. */
  dayKey: string;
  sessions: SessionRecord[];
  tasks: FocusTask[];
  interruptions: Interruption[];
}

function minutes(seconds: number) {
  return Math.max(1, Math.round(seconds / 60));
}

function DayLedgerComponent({ dayKey, sessions, tasks, interruptions }: DayLedgerProps) {
  const isToday = (timestamp: number) => getLocalDateKey(timestamp) === dayKey;
  const today = sessions
    .filter((session) => isToday(session.startedAt))
    .sort((a, b) => b.startedAt - a.startedAt);
  const focus = today.filter(
    (session) => session.phase === "focus" && session.outcome === "completed",
  );
  // The same sum the toolbar shows. Adding up each row's rounded minutes
  // instead let the two totals disagree on the same screen.
  const focusMinutes = Math.round(getCompletedFocusMinutes(focus));
  const planned = tasks
    .filter((task) => !task.done || (task.completedAt !== null && isToday(task.completedAt)))
    .reduce((total, task) => total + task.estimate, 0);
  const todayInterruptions = interruptions.filter((item) => isToday(item.capturedAt));

  return (
    <section className="ledger" aria-labelledby="ledger-heading">
      <details open>
        <summary>
          <span id="ledger-heading">Today</span>
          <span className="ledger-summary">
            {focus.length} focus · {focusMinutes}m · {todayInterruptions.length} interruptions
          </span>
          <ChevronDown className="ledger-chevron" aria-hidden="true" size={18} />
        </summary>
        <div className="capacity-line" aria-label={`${planned} focus sessions planned`}>
          <span>Planned {planned}</span>
          <div className="capacity-ticks" aria-hidden="true">
            {Array.from({ length: Math.min(16, Math.max(planned, 8)) }, (_, index) => (
              <i
                key={index}
                className={`${index < focus.length ? "done" : ""} ${index >= 14 ? "over" : ""}`}
              />
            ))}
          </div>
          <span>{planned > 16 ? "Plan is above 16" : planned > 14 ? "Keep room for overflow" : "12–14 is a full day"}</span>
        </div>
        <div className="session-list" role="list" aria-label="Today's session history">
          {today.length === 0 ? (
            <p className="empty-ledger">Completed sessions will appear here.</p>
          ) : (
            today.slice(0, 12).map((session) => (
              <div className="session-row" role="listitem" key={session.id}>
                <time dateTime={new Date(session.startedAt).toISOString()}>
                  {new Date(session.startedAt).toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </time>
                <span className="session-kind">
                  {session.phase === "focus"
                    ? session.taskTitle || "Focus"
                    : session.phase === "longBreak"
                      ? "Long break"
                      : "Short break"}
                </span>
                <span className={`session-outcome outcome-${session.outcome}`}>
                  {session.outcome}
                </span>
                <span>{minutes(session.durationSeconds)}m</span>
              </div>
            ))
          )}
        </div>
      </details>
    </section>
  );
}

/**
 * Memoised: the window re-renders every second while the timer runs, and this
 * component has nothing to do with the countdown. Its props are stable
 * callbacks and slices of the snapshot, so it only re-renders when something
 * it shows actually changed.
 */
export const DayLedger = memo(DayLedgerComponent);
