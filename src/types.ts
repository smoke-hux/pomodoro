export type Phase = "focus" | "shortBreak" | "longBreak";
export type TimerStatus = "idle" | "running" | "paused";
export type SessionOutcome = "completed" | "skipped" | "abandoned";
export type ThemePreference = "system" | "light" | "dark";

export interface Settings {
  focusMinutes: number;
  shortBreakMinutes: number;
  longBreakMinutes: number;
  roundsBeforeLongBreak: number;
  autoStartBreaks: boolean;
  autoStartFocus: boolean;
  notifications: boolean;
  sound: boolean;
  theme: ThemePreference;
}

export interface TimerState {
  phase: Phase;
  status: TimerStatus;
  durationSeconds: number;
  remainingSeconds: number;
  startedAt: number | null;
  endsAt: number | null;
  activeTaskId: string | null;
  completedInCycle: number;
}

export interface FocusTask {
  id: string;
  title: string;
  estimate: number;
  completedPomodoros: number;
  done: boolean;
  createdAt: number;
  completedAt: number | null;
}

export interface Interruption {
  id: string;
  text: string;
  category: "internal" | "external";
  capturedAt: number;
  handled: boolean;
  taskId: string | null;
}

export interface SessionRecord {
  id: string;
  phase: Phase;
  taskId: string | null;
  taskTitle: string | null;
  durationSeconds: number;
  startedAt: number;
  endedAt: number;
  outcome: SessionOutcome;
}

export interface AppSnapshot {
  settings: Settings;
  timer: TimerState;
  tasks: FocusTask[];
  interruptions: Interruption[];
  sessions: SessionRecord[];
}

export const defaultSnapshot: AppSnapshot = {
  settings: {
    focusMinutes: 25,
    shortBreakMinutes: 5,
    longBreakMinutes: 15,
    roundsBeforeLongBreak: 4,
    autoStartBreaks: true,
    autoStartFocus: false,
    notifications: true,
    sound: true,
    theme: "system",
  },
  timer: {
    phase: "focus",
    status: "idle",
    durationSeconds: 1_500,
    remainingSeconds: 1_500,
    startedAt: null,
    endsAt: null,
    activeTaskId: null,
    completedInCycle: 0,
  },
  tasks: [],
  interruptions: [],
  sessions: [],
};
