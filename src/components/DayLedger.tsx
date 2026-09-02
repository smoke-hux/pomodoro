import { ChevronDown } from "lucide-react";
import type { FocusTask, Interruption, SessionRecord } from "../types";

interface DayLedgerProps {
  sessions: SessionRecord[];
  tasks: FocusTask[];
  interruptions: Interruption[];
}

function isToday(timestamp: number) {
  const date = new Date(timestamp);
  const now = new Date();
  return (
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  );
}

function minutes(seconds: number) {
  return Math.max(1, Math.round(seconds / 60));
}

export function DayLedger({ sessions, tasks, interruptions }: DayLedgerProps) {
  const today = sessions
    .filter((session) => isToday(session.startedAt))
    .sort((a, b) => b.startedAt - a.startedAt);
  const focus = today.filter(
    (session) => session.phase === "focus" && session.outcome === "completed",
  );
  const focusMinutes = focus.reduce(
    (total, session) => total + minutes(session.durationSeconds),
    0,
  );
  const planned = tasks
    .filter((task) => !task.done || isToday(task.completedAt ?? 0))
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
