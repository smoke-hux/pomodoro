export type Phase = "focus" | "shortBreak" | "longBreak";
export type TimerStatus = "idle" | "running" | "paused";
export type SessionOutcome = "completed" | "skipped" | "abandoned";
export type ThemePreference = "system" | "light" | "dark";

/**
 * How the remaining time is drawn. Each face answers "how much is left?" in a
 * different way — read, proportion, count, glance, or words — rather than being
 * a decorative skin over the same numerals.
 */
export type TimerFace =
  | "digits"
  | "ring"
  | "pips"
  | "bar"
  | "words"
  | "analog"
  | "vessel"
  | "arc"
  | "blocks"
  | "orbit";

export interface DesktopNotification {
  id: string;
  appName: string;
  summary: string;
  body: string;
  urgency: 0 | 1 | 2;
  receivedAt: number;
  duringFocus: boolean;
  triaged: boolean;
  /**
   * The id the sender passed to Notify. Non-zero means later calls carrying the
   * same id update this row instead of adding another one.
   */
  replacesId: number;
  /** The task this notification was turned into, so it cannot be turned twice. */
  taskId: string | null;
}

/**
 * Whether the notification monitor is actually running.
 *
 * Capture can be switched on and still fail to start — the session bus may
 * refuse to hand out a monitor, or there may be no session bus at all. The UI
 * reads this rather than the settings toggle, so it never claims to be watching
 * when nothing is.
 */
export type CaptureState = "off" | "starting" | "active" | "failed";

export interface CaptureStatus {
  state: CaptureState;
  /** A D-Bus error message when the state is "failed". Never message text. */
  detail: string;
}

export interface NotificationFilter {
  enabled: boolean;
  minUrgency: 0 | 1 | 2;
  mutedApps: string[];
  priorityApps: string[];
  focusOnly: boolean;
}

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
  timerFace: TimerFace;
  notificationFilter: NotificationFilter;
  /**
   * Turn the desktop's notification banners off for the length of each focus
   * interval and put them back afterwards. Off by default: it changes a setting
   * that belongs to the desktop, not to Pomodoro.
   */
  silenceBannersDuringFocus: boolean;
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
  /** The task this note was turned into, so it cannot be turned twice. */
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
  notifications: DesktopNotification[];
  captureStatus: CaptureStatus;
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
    timerFace: "digits",
    silenceBannersDuringFocus: false,
    // Capture is opt-in. Nothing is watched until the user says so.
    notificationFilter: {
      enabled: false,
      minUrgency: 0,
      mutedApps: [],
      priorityApps: [],
      focusOnly: false,
    },
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
  notifications: [],
  captureStatus: { state: "off", detail: "" },
};
