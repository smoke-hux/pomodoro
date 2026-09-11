// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
    onSave: vi.fn(async () => {}),
    onClearHistory: vi.fn(async () => {}),
    onClearNotifications: vi.fn(async () => {}),
    ...overrides,
  };
  return { props, ...render(<SettingsDialog {...props} />) };
}

describe("unsaved edits", () => {
  it("survives the state broadcast that arrives twice a second", () => {
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
    const onSave = vi.fn<(next: Settings) => Promise<void>>(async () => {});
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
    const onSave = vi.fn<(next: Settings) => Promise<void>>(async () => {});
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
