// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NotificationInbox } from "./NotificationInbox";
import type { CaptureStatus, DesktopNotification } from "../types";

afterEach(cleanup);

function notification(patch: Partial<DesktopNotification> = {}): DesktopNotification {
  return {
    id: "notif-1",
    appName: "Thunderbird",
    summary: "Re: brief review",
    body: "Comments before the standup.",
    urgency: 1,
    receivedAt: Date.now() - 60_000,
    duringFocus: true,
    triaged: false,
    replacesId: 0,
    taskId: null,
    ...patch,
  };
}

function renderInbox(
  notifications: DesktopNotification[],
  captureStatus: CaptureStatus = { state: "active", detail: "" },
  captureEnabled = true,
) {
  const onConvert = vi.fn();
  render(
    <NotificationInbox
      notifications={notifications}
      captureEnabled={captureEnabled}
      captureStatus={captureStatus}
      onTriage={vi.fn()}
      onConvert={onConvert}
      onDelete={vi.fn()}
      onOpenSettings={vi.fn()}
    />,
  );
  return { onConvert };
}

describe("turning a notification into a task", () => {
  it("is offered once", () => {
    const { onConvert } = renderInbox([notification()]);
    const convert = screen.getByRole("button", { name: /Turn into task/i });

    fireEvent.click(convert);
    expect(onConvert).toHaveBeenCalledWith("notif-1");
  });

  it("is not offered again once the notification has a task", () => {
    // Two identical tasks from two clicks was the bug. The row now shows what
    // already happened instead of an action that would repeat it.
    renderInbox([notification({ taskId: "task-1", triaged: true })]);

    expect(screen.queryByRole("button", { name: /Turn into task/i })).toBeNull();
    const done = screen.getByRole("button", { name: /Already a task/i });
    expect((done as HTMLButtonElement).disabled).toBe(true);
  });

  it("is refused for a notification with no summary to name the task", () => {
    renderInbox([notification({ summary: "   " })]);
    const convert = screen.getByRole("button", { name: /Turn into task/i });
    expect((convert as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("capture health", () => {
  it("says nothing while the monitor is working", () => {
    renderInbox([notification()]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("warns in place of pretending the inbox is simply empty", () => {
    renderInbox([], { state: "failed", detail: "no session bus" });
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("nothing is being watched");
    expect(status.textContent).toContain("no session bus");
  });

  it("stays quiet when capture is off, because nothing was promised", () => {
    renderInbox([], { state: "failed", detail: "no session bus" }, false);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("does not claim capture hides the banner", () => {
    renderInbox([]);
    expect(screen.getByText(/Nothing captured yet/i).textContent).toContain(
      "does not stop the banner appearing",
    );
  });
});
