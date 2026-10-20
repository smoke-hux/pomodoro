// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "./SettingsDialog";
import { api } from "../lib/api";
import { defaultSnapshot } from "../types";
import type { CaptureStatus, Settings } from "../types";

vi.mock("../lib/api", () => ({
  api: {
    exportData: vi.fn(),
    exportSessionsCsv: vi.fn(),
    previewImport: vi.fn(),
    confirmImport: vi.fn(),
    cancelImport: vi.fn(),
    openDataLocation: vi.fn(),
  },
}));

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
    onPreviewSound: vi.fn(async () => true),
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

  it("settles what was typed when Enter saves without leaving the field", async () => {
    // Enter submits the form with the field still focused. The save used to
    // take the last in-range value passed up while typing: 15 for 150, and
    // the old value for a 3 below the minimum.
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => true);
    renderDialog({ onSave, settings: settings({ focusMinutes: 25, longBreakMinutes: 15 }) });
    const focus = screen.getByLabelText("Focus") as HTMLInputElement;
    const longBreak = screen.getByLabelText("Long break") as HTMLInputElement;

    fireEvent.focus(longBreak);
    fireEvent.change(longBreak, { target: { value: "" } });
    type(longBreak, "3");
    fireEvent.blur(longBreak);
    fireEvent.focus(focus);
    fireEvent.change(focus, { target: { value: "" } });
    type(focus, "150");
    // What a browser does for Enter in a form field: the key, then the submit.
    fireEvent.keyDown(focus, { key: "Enter" });
    fireEvent.submit(focus.closest("form")!);

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    expect(onSave.mock.calls[0][0]).toMatchObject({ focusMinutes: 120, longBreakMinutes: 5 });
    expect(focus.value).toBe("120");
  });
});

describe("app lists", () => {
  it("keep a name that was typed but never added when the dialog is saved", async () => {
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => true);
    renderDialog({ onSave });
    fireEvent.click(screen.getByLabelText(/Capture desktop notifications/i));
    const muted = screen.getByLabelText("Muted apps") as HTMLInputElement;

    fireEvent.focus(muted);
    fireEvent.change(muted, { target: { value: "  Slack " } });
    // Reaching Save, by mouse or by Tab, takes focus out of the field first.
    fireEvent.blur(muted);
    fireEvent.click(screen.getByRole("button", { name: /Save settings/i }));

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    expect(onSave.mock.calls[0][0].notificationFilter.mutedApps).toEqual(["Slack"]);
    expect(muted.value).toBe("");
  });

  it("add a name once, whether by Enter, by Add, or by leaving the field", () => {
    renderDialog();
    fireEvent.click(screen.getByLabelText(/Capture desktop notifications/i));
    const muted = screen.getByLabelText("Muted apps") as HTMLInputElement;

    fireEvent.change(muted, { target: { value: "Slack" } });
    fireEvent.keyDown(muted, { key: "Enter" });
    fireEvent.blur(muted);
    fireEvent.change(muted, { target: { value: "slack" } });
    fireEvent.blur(muted);

    expect(screen.getAllByRole("button", { name: /Remove .* from muted apps/i })).toHaveLength(1);
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
  it("lists the shortcut for skipping an interval", () => {
    renderDialog();
    const shortcut = screen.getByText("Ctrl S");
    expect(shortcut.closest("div")?.textContent).toContain("Skip interval");
  });

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

describe("data transfer close guards", () => {
  beforeEach(() => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.resetAllMocks();
    vi.mocked(api.cancelImport).mockResolvedValue();
    vi.mocked(api.previewImport).mockResolvedValue({
      token: "chosen-file",
      fileName: "backup.json",
      tasks: 3,
      sessions: 12,
      interruptions: 2,
      notifications: 4,
    });
  });
  afterEach(() => Reflect.deleteProperty(window, "__TAURI_INTERNALS__"));

  async function beginImport() {
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(screen.getByRole("button", { name: "Import and replace" }));
  }

  it("blocks backdrop, header, Cancel and form submission until an import finishes", async () => {
    let finish!: () => void;
    vi.mocked(api.confirmImport).mockImplementation(
      () => new Promise((resolve) => (finish = resolve)),
    );
    const onBusyChange = vi.fn();
    const { props } = renderDialog({ onBusyChange });
    await beginImport();

    const close = screen.getByRole("button", { name: "Close settings" });
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const save = screen.getByRole("button", { name: "Save settings" });
    expect((close as HTMLButtonElement).disabled).toBe(true);
    expect((cancel as HTMLButtonElement).disabled).toBe(true);
    expect((save as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(close);
    fireEvent.click(cancel);
    fireEvent.mouseDown(screen.getByRole("dialog").parentElement!);
    fireEvent.submit(save.closest("form")!);

    expect(props.onClose).not.toHaveBeenCalled();
    expect(props.onSave).not.toHaveBeenCalled();
    expect(api.cancelImport).not.toHaveBeenCalled();
    expect(onBusyChange).toHaveBeenLastCalledWith(true);

    await act(async () => finish());
    await waitFor(() => expect(props.onClose).toHaveBeenCalledOnce());
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
  });

  it("restores dismissal and keeps the preview when an import fails", async () => {
    let fail!: (reason: string) => void;
    vi.mocked(api.confirmImport).mockImplementation(
      () => new Promise((_, reject) => (fail = reject)),
    );
    const { props } = renderDialog();
    await beginImport();
    await act(async () => fail("The recovery copy could not be saved."));

    expect((await screen.findByRole("alert")).textContent).toContain("could not be saved");
    expect(screen.getByRole("heading", { name: "Import backup.json" })).toBeTruthy();
    expect(props.onClose).not.toHaveBeenCalled();
    const close = screen.getByRole("button", { name: "Close settings" });
    expect((close as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(close);
    expect(props.onClose).toHaveBeenCalledOnce();
  });

  it("keeps settings open if an earlier save returns during a data transfer", async () => {
    let finishSave!: (saved: boolean) => void;
    let finishImport!: () => void;
    const onSave = vi.fn(() => new Promise<boolean>((resolve) => (finishSave = resolve)));
    vi.mocked(api.confirmImport).mockImplementation(
      () => new Promise((resolve) => (finishImport = resolve)),
    );
    const { props } = renderDialog({ onSave });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await beginImport();
    await act(async () => finishSave(true));

    expect(props.onClose).not.toHaveBeenCalled();
    await act(async () => finishImport());
    await waitFor(() => expect(props.onClose).toHaveBeenCalledOnce());
  });
});

describe("sound", () => {
  it("can be heard before it is relied on, without saving or closing anything", async () => {
    let finished: (played: boolean) => void = () => {};
    const onPreviewSound = vi.fn(() => new Promise<boolean>((resolve) => (finished = resolve)));
    const onSave = vi.fn<(next: Settings) => Promise<boolean>>(async () => true);
    const onClose = vi.fn();
    renderDialog({ onPreviewSound, onSave, onClose });

    fireEvent.click(screen.getByRole("button", { name: "Test sound" }));
    expect(onPreviewSound).toHaveBeenCalledTimes(1);
    // The backend answers only once the sound has played, so meanwhile the
    // button says so and cannot start a second one.
    const playing = screen.getByRole("button", { name: "Playing…" }) as HTMLButtonElement;
    expect(playing.disabled).toBe(true);

    finished(true);
    await waitFor(() => expect(screen.getByRole("button", { name: "Test sound" })).toBeTruthy());
    // A plain button, not a submit: trying the sound must not save the draft.
    expect(onSave).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
