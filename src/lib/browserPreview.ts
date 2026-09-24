import { defaultSnapshot } from "../types";
import type { AppSnapshot } from "../types";
import { readRememberedTheme } from "./theme";

export function browserPreview(): AppSnapshot {
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
      // The preview has no backend, so the remembered theme is its setting.
      theme: readRememberedTheme(),
      notificationFilter: {
        ...defaultSnapshot.settings.notificationFilter,
        enabled: true,
      },
    },
    timer: { ...defaultSnapshot.timer, activeTaskId: "preview-1" },
  };
}
