// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "./SettingsDialog";
import { defaultSnapshot } from "../types";
import type { CaptureStatus, Settings } from "../types";

afterEach(cleanup);

const OFF: CaptureStatus = { state: "off", detail: "" };

function settings(patch: Partial<Settings> = {}): Settings {
  return { ...defaultSnapshot.settings, ...patch };
}

function renderDialog(overrides: Partial<Parameters<typeof SettingsDialog>[0]> = {}) {
  const props = {
    open: true,
    settings: settings(),
    captureStatus: OFF,
    notificationCount: 0,
    onClose: vi.fn(),
    onSave: vi.fn(async () => true),
    onClearHistory: vi.fn(async () => {}),
    onClearNotifications: vi.fn(async () => {}),
    ...overrides,
  };
  return { props, ...render(<SettingsDialog {...props} />) };
}

describe("unsaved edits", () => {
  it("survives a state broadcast arriving mid-edit", () => {
    // The backend re-emits the whole snapshot while the timer runs, so
    // `settings` is a new object on every tick even when nothing in it changed.
    const { props, rerender } = renderDialog();
    const focus = screen.getByLabelText("Focus") as HTMLInputElement;

    fireEvent.change(focus, { target: { value: "50" } });
    expect(focus.value).toBe("50");

    for (let tick = 0; tick < 4; tick += 1) {
      rerender(<SettingsDialog {...props} settings={settings()} />);
    }

    expect((screen.getByLabelText("Focus") as HTMLInputElement).value).toBe("50");
  });

  it("is discarded and reseeded when the dialog is reopened", () => {
    const { props, rerender } = renderDialog();
    fireEvent.change(screen.getByLabelText("Focus"), { target: { value: "50" } });

    rerender(<SettingsDialog {...props} open={false} />);
    rerender(<SettingsDialog {...props} open settings={settings({ focusMinutes: 30 })} />);

    // Reopening shows what is actually saved, not the abandoned edit.
    expect((screen.getByLabelText("Focus") as HTMLInputElement).value).toBe("30");
  });

  it("saves exactly what was edited", async () => {
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => true);
    renderDialog({ onSave });

    fireEvent.change(screen.getByLabelText("Focus"), { target: { value: "45" } });
    fireEvent.click(screen.getByText("Save settings"));

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0]).toMatchObject({ focusMinutes: 45 });
  });
});

describe("capture health", () => {
  const enabled = settings({
    notificationFilter: { ...defaultSnapshot.settings.notificationFilter, enabled: true },
  });

  it("says nothing while capture is off and idle", () => {
    renderDialog();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says nothing while capture is on and working", () => {
    renderDialog({ settings: enabled, captureStatus: { state: "active", detail: "" } });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("warns when the toggle is on but the monitor failed", () => {
    renderDialog({
      settings: enabled,
      captureStatus: { state: "failed", detail: "BecomeMonitor was refused" },
    });
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("nothing is being watched");
    expect(status.textContent).toContain("BecomeMonitor was refused");
    expect(status.className).toContain("warning");
  });

  it("warns when the saved filter is on but no monitor is running at all", () => {
    renderDialog({ settings: enabled, captureStatus: OFF });
    expect(screen.getByRole("status").textContent).toContain("not running");
  });

  it("does not warn on a toggle the user has only just ticked", () => {
    // The saved filter is still off, so the monitor being off is correct.
    renderDialog({ captureStatus: OFF });
    fireEvent.click(screen.getByLabelText(/Capture desktop notifications/i, { exact: false }));
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("silencing banners", () => {
  it("is off by default and is saved when turned on", () => {
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => true);
    renderDialog({ onSave });

    const toggle = screen.getByRole("checkbox", {
      name: /Silence banners during focus/i,
    }) as HTMLInputElement;
    expect(toggle.checked).toBe(false);

    fireEvent.click(toggle);
    fireEvent.click(screen.getByText("Save settings"));
    expect(onSave.mock.calls[0][0]).toMatchObject({ silenceBannersDuringFocus: true });
  });

  it("stays available when capture itself is off, because it is a separate thing", () => {
    renderDialog();
    const toggle = screen.getByRole("checkbox", {
      name: /Silence banners during focus/i,
    }) as HTMLInputElement;
    expect(toggle.disabled).toBe(false);
  });
});

describe("the capture explanation", () => {
  it("does not claim that watching hides anything", () => {
    renderDialog();
    const note = screen.getByText(/Pomodoro can watch the desktop notification service/i);
    expect(note.textContent).toContain("banners still appear");
  });
});

describe("number fields", () => {
  /**
   * Types into a field one key at a time, the way a keyboard does: each key is
   * added to whatever the field is showing by then, which is where rewriting
   * the value on every keystroke does its damage.
   */
  function type(field: HTMLInputElement, keys: string) {
    fireEvent.focus(field);
    for (const key of keys) {
      fireEvent.change(field, { target: { value: field.value + key } });
    }
  }

  it("lets a value be typed whose first digit is below the minimum", async () => {
    // Long break has a minimum of 5. Clamping each keystroke turned the "1" of
    // "15" into 5, and the "5" that followed made 55.
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => true);
    renderDialog({ onSave });
    const field = screen.getByLabelText("Long break") as HTMLInputElement;

    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: "" } });
    type(field, "15");
    expect(field.value).toBe("15");
    fireEvent.blur(field);

    fireEvent.click(screen.getByRole("button", { name: /Save settings/i }));
    await waitFor(() => expect(onSave).toHaveBeenCalled());
    expect(onSave.mock.calls[0][0]).toMatchObject({ longBreakMinutes: 15 });
  });

  it("can be emptied to retype, and settles into range when left", () => {
    renderDialog({ settings: settings({ focusMinutes: 25 }) });
    const field = screen.getByLabelText("Focus") as HTMLInputElement;

    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: "" } });
    expect(field.value).toBe("");
    type(field, "300");
    fireEvent.blur(field);
    expect(field.value).toBe("120");

    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: "" } });
    fireEvent.blur(field);
    // Left empty, it goes back to what it was rather than to the minimum.
    expect(field.value).toBe("120");
  });
});

describe("saving", () => {
  it("keeps the dialog and the edits open when the save is refused", async () => {
    const onClose = vi.fn();
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => false);
    renderDialog({ onSave, onClose });
    fireEvent.change(screen.getByLabelText("Focus"), { target: { value: "45" } });
    fireEvent.click(screen.getByRole("button", { name: /Save settings/i }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    expect(onClose).not.toHaveBeenCalled();
    expect((screen.getByLabelText("Focus") as HTMLInputElement).value).toBe("45");
  });

  it("is sent once when submitted twice before it returns", async () => {
    let finish: (saved: boolean) => void = () => {};
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(
      () => new Promise((resolve) => (finish = resolve)),
    );
    const onClose = vi.fn();
    renderDialog({ onSave, onClose });
    const save = screen.getByRole("button", { name: /Save settings/i });
    fireEvent.click(save);
    fireEvent.click(save);
    expect(onSave).toHaveBeenCalledTimes(1);

    finish(true);
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});

describe("keyboard", () => {
  it("takes focus when it opens, so keys no longer reach what is behind it", async () => {
    const behind = document.createElement("button");
    document.body.append(behind);
    behind.focus();
    const { container } = renderDialog();

    await waitFor(() => expect(container.contains(document.activeElement)).toBe(true));
    behind.remove();
  });

  it("wraps Tab at both ends instead of walking out into the page", async () => {
    const { container } = renderDialog();
    await waitFor(() => expect(container.contains(document.activeElement)).toBe(true));
    const close = screen.getByRole("button", { name: "Close settings" });
    const save = screen.getByRole("button", { name: /Save settings/i });

    save.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(close);

    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(save);
  });
});
