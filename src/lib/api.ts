import { invoke } from "@tauri-apps/api/core";
import type {
  AppSnapshot,
  NotificationFilter,
  Phase,
  Settings,
} from "../types";

type CommandArgs = Record<string, unknown>;

export async function command(
  name: string,
  args: CommandArgs = {},
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>(name, args);
}

export const api = {
  snapshot: () => command("get_snapshot"),
  toggleTimer: () => command("toggle_timer"),
  resetTimer: () => command("reset_timer"),
  skipPhase: () => command("skip_phase"),
  setPhase: (phase: Phase) => command("set_phase", { phase }),
  selectTask: (taskId: string | null) => command("select_task", { taskId }),
  addTask: (title: string, estimate: number) =>
    command("add_task", { title, estimate }),
  updateTask: (id: string, title: string, estimate: number) =>
    command("update_task", { id, title, estimate }),
  toggleTask: (id: string) => command("toggle_task", { id }),
  deleteTask: (id: string) => command("delete_task", { id }),
  captureInterruption: (
    text: string,
    category: "internal" | "external",
  ) => command("capture_interruption", { text, category }),
  setInterruptionHandled: (id: string, handled: boolean) =>
    command("set_interruption_handled", { id, handled }),
  deleteInterruption: (id: string) =>
    command("delete_interruption", { id }),
  convertInterruption: (id: string) =>
    command("convert_interruption_to_task", { id }),
  updateSettings: (settings: Settings) =>
    command("update_settings", { settings }),
  clearHistory: () => command("clear_history"),
  setNotificationFilter: (filter: NotificationFilter) =>
    command("set_notification_filter", { filter }),
  triageNotification: (id: string, triaged: boolean) =>
    command("triage_notification", { id, triaged }),
  convertNotification: (id: string) =>
    command("convert_notification", { id }),
  deleteNotification: (id: string) =>
    command("delete_notification", { id }),
  clearNotifications: () => command("clear_notifications"),
};
