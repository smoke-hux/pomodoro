import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot, ImportPreview, Phase, Settings } from "../types";

type CommandArgs = Record<string, unknown>;

/**
 * Runs a command that changes state.
 *
 * It resolves when the backend has applied and saved the change; the new
 * state arrives on the `state-changed` event, which the backend emits once per
 * change. Commands used to return the whole snapshot as well, so every click
 * serialised the entire store twice and the window had to guess which of the
 * two copies was newer.
 */
export async function command(name: string, args: CommandArgs = {}): Promise<void> {
  await invoke<void>(name, args);
}

export const api = {
  /** The one read. Asked once on load; everything after comes as events. */
  snapshot: () => invoke<AppSnapshot>("get_snapshot"),
  toggleTimer: () => command("toggle_timer"),
  resetTimer: () => command("reset_timer"),
  skipPhase: () => command("skip_phase"),
  setPhase: (phase: Phase) => command("set_phase", { phase }),
  selectTask: (taskId: string | null) => command("select_task", { taskId }),
  addTask: (title: string, estimate: number) => command("add_task", { title, estimate }),
  updateTask: (id: string, title: string, estimate: number) =>
    command("update_task", { id, title, estimate }),
  toggleTask: (id: string) => command("toggle_task", { id }),
  deleteTask: (id: string) => command("delete_task", { id }),
  captureInterruption: (text: string, category: "internal" | "external") =>
    command("capture_interruption", { text, category }),
  setInterruptionHandled: (id: string, handled: boolean) =>
    command("set_interruption_handled", { id, handled }),
  deleteInterruption: (id: string) => command("delete_interruption", { id }),
  convertInterruption: (id: string) => command("convert_interruption_to_task", { id }),
  updateSettings: (settings: Settings) => command("update_settings", { settings }),
  /** Plays the interval-finished sound once, whatever the sound setting says. */
  previewSound: () => command("preview_sound"),
  clearHistory: () => command("clear_history"),
  triageNotification: (id: string, triaged: boolean) =>
    command("triage_notification", { id, triaged }),
  convertNotification: (id: string) => command("convert_notification", { id }),
  deleteNotification: (id: string) => command("delete_notification", { id }),
  clearNotifications: () => command("clear_notifications"),
  exportData: () => invoke<string | null>("export_data"),
  exportSessionsCsv: () => invoke<string | null>("export_sessions_csv"),
  previewImport: () => invoke<ImportPreview | null>("preview_import"),
  confirmImport: (token: string) => command("confirm_import", { token }),
  cancelImport: (token: string) => command("cancel_import", { token }),
  openDataLocation: () => command("open_data_location"),
};
