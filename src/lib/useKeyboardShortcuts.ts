import { useEffect } from "react";
import type { Phase } from "../types";

function isTextEntry(target: EventTarget | null) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

/**
 * True when Space is already the focused element's own key.
 *
 * Space is the timer's shortcut, but it is also how a keyboard user presses the
 * button they have just tabbed to. The window-level handler used to swallow it
 * either way, so tabbing to "Skip" and pressing Space started the timer and left
 * the button untouched — the control looked focused and did nothing.
 */
export function activatesOnSpace(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target instanceof HTMLButtonElement || target instanceof HTMLAnchorElement) return true;
  if (target instanceof HTMLSelectElement || target instanceof HTMLTextAreaElement) return true;
  if (target instanceof HTMLInputElement) return true;
  // <summary> opens its <details> on Space; the row menus are built from them.
  if (target.tagName === "SUMMARY") return true;
  const role = target.getAttribute("role");
  return (
    role === "button" ||
    role === "checkbox" ||
    role === "radio" ||
    role === "switch" ||
    role === "tab" ||
    role === "option" ||
    role === "menuitem"
  );
}

interface KeyboardShortcuts {
  captureOpen: boolean;
  settingsOpen: boolean;
  closeCapture: () => void;
  closeSettings: () => void;
  closeSidebar: () => void;
  openCapture: () => void;
  openSettings: () => void;
  openAddTask: () => void;
  toggleTimer: () => void;
  selectPhase: (phase: Phase) => void;
}

export function useKeyboardShortcuts({
  captureOpen,
  settingsOpen,
  closeCapture,
  closeSettings,
  closeSidebar,
  openCapture,
  openSettings,
  openAddTask,
  toggleTimer,
  selectPhase,
}: KeyboardShortcuts) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // An open row menu is the innermost thing on screen, so it goes first
        // and focus returns to the button that opened it.
        const menu = document.querySelector<HTMLDetailsElement>("details.row-menu[open]");
        if (menu && !captureOpen && !settingsOpen) {
          menu.open = false;
          menu.querySelector("summary")?.focus();
          return;
        }
        if (captureOpen) closeCapture();
        else if (settingsOpen) closeSettings();
        else closeSidebar();
        return;
      }
      // A modal owns the keyboard while it is open. Without this guard the
      // window-level handler still fires underneath it, so Space on a dialog
      // button would both press the button and toggle the timer behind it.
      if (captureOpen || settingsOpen) return;
      if (isTextEntry(event.target)) return;
      if (event.code === "Space") {
        // A held key repeats, and every repeat was another start or pause —
        // dozens of saves, ending in whichever state the last one landed on.
        // With a modifier it is somebody else's shortcut, not this one.
        if (event.repeat || event.ctrlKey || event.altKey || event.metaKey) return;
        // The focused control gets its own key back.
        if (activatesOnSpace(event.target)) return;
        event.preventDefault();
        toggleTimer();
      } else if (event.ctrlKey && event.key.toLowerCase() === "i") {
        event.preventDefault();
        openCapture();
      } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        openAddTask();
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        openSettings();
      } else if (event.ctrlKey && ["1", "2", "3"].includes(event.key)) {
        event.preventDefault();
        const phases: Phase[] = ["focus", "shortBreak", "longBreak"];
        selectPhase(phases[Number(event.key) - 1]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    captureOpen,
    closeCapture,
    closeSettings,
    closeSidebar,
    openAddTask,
    openCapture,
    openSettings,
    selectPhase,
    settingsOpen,
    toggleTimer,
  ]);
}
