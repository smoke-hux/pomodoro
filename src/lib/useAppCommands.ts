import { useCallback } from "react";
import { api } from "./api";
import type { Phase, Settings } from "../types";

/** Stable callbacks keep memoized panels independent of the countdown. */
export function useAppCommands(
  inTauri: boolean,
  timerPhase: Phase,
  showNotice: (message: string) => void,
  closeSidebar: () => void,
) {
  // A command resolves once the backend has applied and saved it. The new state
  // arrives on "state-changed" — the single channel for every change, whether
  // it came from this window, the tray, or the clock — so there is nothing to
  // reconcile between a returned value and a broadcast one.
  const run = useCallback(
    async (action: () => Promise<void>, successMessage?: string): Promise<boolean> => {
      if (!inTauri) {
        showNotice("Desktop controls are active in the packaged Ubuntu app.");
        return false;
      }
      try {
        await action();
        if (successMessage) showNotice(successMessage);
        return true;
      } catch (error) {
        showNotice(typeof error === "string" ? error : "That action could not be completed.");
        return false;
      }
    },
    [inTauri, showNotice],
  );

  const selectPhase = useCallback((phase: Phase) => void run(() => api.setPhase(phase)), [run]);

  // Every handler below is stable across renders, which is what lets the
  // memoised sidebar, ledger and dialogs sit out the once-a-second countdown.
  const toggleTimer = useCallback(() => void run(api.toggleTimer), [run]);
  const resetTimer = useCallback(() => void run(api.resetTimer, "Interval reset."), [run]);
  const skipPhase = useCallback(
    () =>
      void run(
        api.skipPhase,
        timerPhase === "focus" ? "Focus ended without credit." : "Break skipped.",
      ),
    [run, timerPhase],
  );
  const selectTask = useCallback(
    (id: string) => {
      void run(() => api.selectTask(id));
      closeSidebar();
    },
    [run, closeSidebar],
  );
  const addTask = useCallback(
    (title: string, estimate: number) => run(() => api.addTask(title, estimate), "Task added."),
    [run],
  );
  const updateTask = useCallback(
    (id: string, title: string, estimate: number) =>
      run(() => api.updateTask(id, title, estimate), "Task updated."),
    [run],
  );
  const toggleTask = useCallback((id: string) => void run(() => api.toggleTask(id)), [run]);
  const deleteTask = useCallback(
    (id: string) => void run(() => api.deleteTask(id), "Task deleted."),
    [run],
  );
  const handleInterruption = useCallback(
    (id: string, handled: boolean) =>
      void run(
        () => api.setInterruptionHandled(id, handled),
        handled ? "Marked handled." : "Moved back to the inbox.",
      ),
    [run],
  );
  const convertInterruption = useCallback(
    (id: string) => void run(() => api.convertInterruption(id), "Added to today’s tasks."),
    [run],
  );
  const deleteInterruption = useCallback(
    (id: string) => void run(() => api.deleteInterruption(id), "Interruption deleted."),
    [run],
  );
  const triageNotification = useCallback(
    (id: string, triaged: boolean) =>
      void run(
        () => api.triageNotification(id, triaged),
        triaged ? "Marked triaged." : "Moved back to pending.",
      ),
    [run],
  );
  const convertNotification = useCallback(
    (id: string) => void run(() => api.convertNotification(id), "Added to today’s tasks."),
    [run],
  );
  const deleteNotification = useCallback(
    (id: string) => void run(() => api.deleteNotification(id), "Notification deleted."),
    [run],
  );
  const saveInterruption = useCallback(
    (text: string, category: "internal" | "external") =>
      run(() => api.captureInterruption(text, category), "Saved. Return to your focus."),
    [run],
  );
  const saveSettings = useCallback(
    (settings: Settings) => run(() => api.updateSettings(settings), "Settings saved."),
    [run],
  );
  // Resolves once the sound has played or failed: the backend answers only
  // then, so the button can say it is playing and the failure, if any, is
  // shown as a notice.
  const previewSound = useCallback(() => run(api.previewSound), [run]);
  const clearHistory = useCallback(async () => {
    await run(api.clearHistory, "Session history cleared.");
  }, [run]);
  const clearNotifications = useCallback(async () => {
    await run(api.clearNotifications, "Captured notifications cleared.");
  }, [run]);

  return {
    run,
    selectPhase,
    toggleTimer,
    resetTimer,
    skipPhase,
    selectTask,
    addTask,
    updateTask,
    toggleTask,
    deleteTask,
    handleInterruption,
    convertInterruption,
    deleteInterruption,
    triageNotification,
    convertNotification,
    deleteNotification,
    saveInterruption,
    saveSettings,
    previewSound,
    clearHistory,
    clearNotifications,
  };
}
