import type { FocusTask, Interruption, SessionRecord } from "../types";

export interface LocalDayBounds {
  /** Local midnight at the beginning of the day, inclusive. */
  start: number;
  /** Local midnight at the beginning of the next day, exclusive. */
  end: number;
}

export interface InterruptionCounts {
  total: number;
  internal: number;
  external: number;
  handled: number;
  unhandled: number;
}

export interface TaskActual {
  taskId: string;
  title: string;
  estimate: number;
  actual: number;
  done: boolean;
}

export interface CompletedFocusDay {
  /** YYYY-MM-DD in the user's local timezone. */
  date: string;
  dayStart: number;
  count: number;
}

function asValidDate(timestamp: number): Date {
  const date = new Date(timestamp);

  if (!Number.isFinite(timestamp) || Number.isNaN(date.getTime())) {
    throw new RangeError("Expected a valid timestamp");
  }

  return date;
}

function padTwo(value: number): string {
  return String(value).padStart(2, "0");
}

function wholeSeconds(value: number): number {
  if (!Number.isFinite(value) || value <= 0) {
    return 0;
  }

  return Math.ceil(value);
}

function isCompletedFocus(session: SessionRecord): boolean {
  return session.phase === "focus" && session.outcome === "completed";
}

export function getLocalDateKey(timestamp: number): string {
  const date = asValidDate(timestamp);
  return `${date.getFullYear()}-${padTwo(date.getMonth() + 1)}-${padTwo(date.getDate())}`;
}

export function getLocalDayBounds(timestamp: number): LocalDayBounds {
  const date = asValidDate(timestamp);
  const start = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const end = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).getTime();

  return { start, end };
}

/** The local day that a key from getLocalDateKey names. */
export function getDayBoundsForKey(dayKey: string): LocalDayBounds {
  const [year, month, day] = dayKey.split("-").map(Number);
  return getLocalDayBounds(new Date(year, month - 1, day).getTime());
}

/**
 * Whether a stored timestamp falls on a day.
 *
 * A comparison, so it cannot throw. It runs while rendering, over timestamps
 * read from the store, and one out-of-range value there — a hand-edited file, a
 * clock that was wrong when it was written — must leave that item out of
 * "today", not take the window down. Formatting each one as a date key did
 * exactly that, and did the formatting for every item on every broadcast.
 */
export function isWithinDay(timestamp: number | null, day: LocalDayBounds): boolean {
  return timestamp !== null && timestamp >= day.start && timestamp < day.end;
}

/** The minutes one session is shown as: rounded, and never less than one. */
export function getSessionMinutes(durationSeconds: number): number {
  return Math.max(1, Math.round(durationSeconds / 60));
}

/**
 * Completed focus time as the sum of what each session is shown as, so the
 * ledger's rows add up to its total and the toolbar agrees with both.
 */
export function getCompletedFocusDisplayMinutes(sessions: readonly SessionRecord[]): number {
  return sessions.reduce(
    (total, session) =>
      total + (isCompletedFocus(session) ? getSessionMinutes(session.durationSeconds) : 0),
    0,
  );
}

export function getTodayFocusSessions(
  sessions: readonly SessionRecord[],
  now: number,
): SessionRecord[] {
  const { start, end } = getLocalDayBounds(now);

  return sessions.filter(
    (session) =>
      session.phase === "focus" && session.startedAt >= start && session.startedAt < end,
  );
}

export function getCompletedFocusCount(sessions: readonly SessionRecord[]): number {
  return sessions.reduce((total, session) => total + Number(isCompletedFocus(session)), 0);
}

export function getCompletedFocusMinutes(sessions: readonly SessionRecord[]): number {
  const completedSeconds = sessions.reduce(
    (total, session) => total + (isCompletedFocus(session) ? session.durationSeconds : 0),
    0,
  );

  return completedSeconds / 60;
}

export function getInterruptionCounts(
  interruptions: readonly Interruption[],
): InterruptionCounts {
  return interruptions.reduce<InterruptionCounts>(
    (counts, interruption) => {
      counts.total += 1;
      counts[interruption.category] += 1;
      counts[interruption.handled ? "handled" : "unhandled"] += 1;
      return counts;
    },
    { total: 0, internal: 0, external: 0, handled: 0, unhandled: 0 },
  );
}

export function getTodayInterruptionCounts(
  interruptions: readonly Interruption[],
  now: number,
): InterruptionCounts {
  const { start, end } = getLocalDayBounds(now);
  return getInterruptionCounts(
    interruptions.filter(
      (interruption) =>
        interruption.capturedAt >= start && interruption.capturedAt < end,
    ),
  );
}

export function getTaskPlannedTotal(tasks: readonly FocusTask[]): number {
  return tasks.reduce((total, task) => total + task.estimate, 0);
}

export function getCompletedFocusByTask(
  sessions: readonly SessionRecord[],
): Record<string, number> {
  return sessions.reduce<Record<string, number>>((actuals, session) => {
    if (isCompletedFocus(session) && session.taskId !== null) {
      actuals[session.taskId] = (actuals[session.taskId] ?? 0) + 1;
    }

    return actuals;
  }, {});
}

export function getTaskActuals(
  tasks: readonly FocusTask[],
  sessions: readonly SessionRecord[],
): TaskActual[] {
  const actuals = getCompletedFocusByTask(sessions);

  return tasks.map((task) => ({
    taskId: task.id,
    title: task.title,
    estimate: task.estimate,
    actual: actuals[task.id] ?? 0,
    done: task.done,
  }));
}

export function getSevenDayCompletedFocusSeries(
  sessions: readonly SessionRecord[],
  now: number,
): CompletedFocusDay[] {
  const reference = asValidDate(now);
  const series: CompletedFocusDay[] = [];

  for (let daysAgo = 6; daysAgo >= 0; daysAgo -= 1) {
    const dayStart = new Date(
      reference.getFullYear(),
      reference.getMonth(),
      reference.getDate() - daysAgo,
    ).getTime();

    series.push({
      date: getLocalDateKey(dayStart),
      dayStart,
      count: 0,
    });
  }

  const pointsByDate = new Map(series.map((point) => [point.date, point]));

  for (const session of sessions) {
    if (!isCompletedFocus(session)) {
      continue;
    }

    const point = pointsByDate.get(getLocalDateKey(session.startedAt));
    if (point) {
      point.count += 1;
    }
  }

  return series;
}

export function formatDuration(totalSeconds: number): string {
  const seconds = wholeSeconds(totalSeconds);
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const remainder = seconds % 60;
  const parts: string[] = [];

  if (hours > 0) {
    parts.push(`${hours}h`);
  }
  if (minutes > 0) {
    parts.push(`${minutes}m`);
  }
  if (remainder > 0 || parts.length === 0) {
    parts.push(`${remainder}s`);
  }

  return parts.join(" ");
}

export function formatCountdown(remainingSeconds: number): string {
  const seconds = wholeSeconds(remainingSeconds);
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const remainder = seconds % 60;

  if (hours > 0) {
    return `${hours}:${padTwo(minutes)}:${padTwo(remainder)}`;
  }

  return `${padTwo(minutes)}:${padTwo(remainder)}`;
}

export function formatClock(timestamp: number): string {
  const date = asValidDate(timestamp);
  return `${padTwo(date.getHours())}:${padTwo(date.getMinutes())}`;
}

// Coarse on purpose. A captured notification is triaged after the interval, so
// the useful question is "roughly when", not "how many seconds ago".
/**
 * A timestamp for a `<time dateTime>` attribute, or nothing if it is not one.
 * `toISOString` throws on an invalid date, and this runs while rendering.
 */
export function toIsoTime(timestamp: number): string | undefined {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

export function formatRelativeTime(timestamp: number, now: number): string {
  const elapsed = now - timestamp;

  if (!Number.isFinite(elapsed) || elapsed < 60_000) {
    return "just now";
  }
  if (elapsed < 3_600_000) {
    return `${Math.floor(elapsed / 60_000)}m ago`;
  }
  if (elapsed < 86_400_000) {
    return `${Math.floor(elapsed / 3_600_000)}h ago`;
  }

  return `${Math.floor(elapsed / 86_400_000)}d ago`;
}
