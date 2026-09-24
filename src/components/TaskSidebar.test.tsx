// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TaskSidebar } from "./TaskSidebar";
import { getLocalDateKey } from "../lib/metrics";
import type { FocusTask } from "../types";

afterEach(cleanup);

const NOW = new Date(2026, 8, 17, 10, 0, 0).getTime();
const DAY = 24 * 60 * 60 * 1_000;

function task(patch: Partial<FocusTask> = {}): FocusTask {
  return {
    id: "task-1",
    title: "Write the report",
    estimate: 2,
    completedPomodoros: 0,
    done: false,
    createdAt: NOW,
    completedAt: null,
    ...patch,
  };
}

function renderSidebar(tasks: FocusTask[], accepted = true) {
  const onUpdateTask = vi.fn().mockResolvedValue(accepted);
  const onDeleteTask = vi.fn();
  const sidebar = (addRequest: number) => (
    <TaskSidebar
      tasks={tasks}
      interruptions={[]}
      notifications={[]}
      captureEnabled={false}
      captureStatus={{ state: "off", detail: "" }}
      activeTaskId={null}
      addRequest={addRequest}
      selectionLocked={false}
      dayKey={getLocalDateKey(NOW)}
      onSelectTask={vi.fn()}
      onAddTask={vi.fn().mockResolvedValue(true)}
      onUpdateTask={onUpdateTask}
      onToggleTask={vi.fn()}
      onDeleteTask={onDeleteTask}
      onOpenCapture={vi.fn()}
      onHandleInterruption={vi.fn()}
      onConvertInterruption={vi.fn()}
      onDeleteInterruption={vi.fn()}
      onTriageNotification={vi.fn()}
      onConvertNotification={vi.fn()}
      onDeleteNotification={vi.fn()}
      onOpenSettings={vi.fn()}
    />
  );
  const { rerender } = render(sidebar(0));
  const requestAdd = (count: number) => rerender(sidebar(count));
  return { onUpdateTask, onDeleteTask, requestAdd };
}

describe("the add shortcut", () => {
  it("takes focus back to the form when the form is already open", () => {
    const { requestAdd } = renderSidebar([task()]);
    requestAdd(1);
    const title = screen.getByLabelText("Task") as HTMLInputElement;
    expect(document.activeElement).toBe(title);

    // Focus goes elsewhere, and Ctrl+N is pressed again.
    fireEvent.change(title, { target: { value: "Draft" } });
    title.blur();
    expect(document.activeElement).not.toBe(title);
    requestAdd(2);

    expect(document.activeElement).toBe(title);
    expect(title.value).toBe("Draft");
    expect(title.selectionStart).toBe(title.selectionEnd);
  });
});

describe("editing a task", () => {
  it("opens the task's own title and estimate, and saves what was changed", async () => {
    const { onUpdateTask } = renderSidebar([task()]);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));

    const title = screen.getByLabelText("Task") as HTMLInputElement;
    expect(title.value).toBe("Write the report");
    fireEvent.change(title, { target: { value: "  Write the quarterly report " } });
    fireEvent.click(screen.getByRole("button", { name: "Increase estimate" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(onUpdateTask).toHaveBeenCalledWith("task-1", "Write the quarterly report", 3),
    );
    // Accepted, so the form gives the row back.
    await waitFor(() => expect(screen.queryByRole("button", { name: "Save" })).toBeNull());
  });

  it("keeps the form and what was typed when the change is refused", async () => {
    const { onUpdateTask } = renderSidebar([task()], false);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.change(screen.getByLabelText("Task"), { target: { value: "Rewritten" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onUpdateTask).toHaveBeenCalled());
    const save = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
    await waitFor(() => expect(save.disabled).toBe(false));
    expect((screen.getByLabelText("Task") as HTMLInputElement).value).toBe("Rewritten");
  });

  it("is abandoned with Escape without saving", () => {
    const { onUpdateTask } = renderSidebar([task()]);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.keyDown(screen.getByLabelText("Task"), { key: "Escape" });

    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(onUpdateTask).not.toHaveBeenCalled();
  });
});

describe("editing two tasks", () => {
  it("keeps the first draft when a second row is opened for editing", () => {
    renderSidebar([task(), task({ id: "task-2", title: "Review notes" })]);
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    fireEvent.change(screen.getByLabelText("Task"), { target: { value: "Half-typed title" } });

    // The remaining row's menu: a single editor used to be replaced here.
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));

    const titles = (screen.getAllByLabelText("Task") as HTMLInputElement[]).map(
      (input) => input.value,
    );
    expect(titles).toEqual(["Half-typed title", "Review notes"]);
  });
});

describe("completed tasks", () => {
  it("lists under Completed today only what was completed today", () => {
    renderSidebar([
      task({ id: "a", title: "Done this morning", done: true, completedAt: NOW - 3_600_000 }),
      task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
    ]);

    expect(screen.getByText("Completed today (1)")).toBeTruthy();
    expect(screen.getByText("Completed earlier (1)")).toBeTruthy();
  });

  it("can be deleted, but only after asking once", () => {
    vi.useFakeTimers();
    try {
      const { onDeleteTask } = renderSidebar([
        task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
      ]);
      // One stray click next to "reopen" must not remove the record.
      fireEvent.click(screen.getByRole("button", { name: "Delete Done last week" }));
      expect(onDeleteTask).not.toHaveBeenCalled();

      act(() => void vi.advanceTimersByTime(1_000));
      fireEvent.click(screen.getByRole("button", { name: /Confirm deleting Done last week/ }));
      expect(onDeleteTask).toHaveBeenCalledWith("b");
    } finally {
      vi.useRealTimers();
    }
  });

  it("is not deleted by the second half of the double click that asked, and shows it is not ready", () => {
    // The confirm button appears under the pointer and takes focus, so a double
    // click, or Enter held a moment too long, used to answer its own question.
    vi.useFakeTimers();
    try {
      const { onDeleteTask } = renderSidebar([
        task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
      ]);
      fireEvent.click(screen.getByRole("button", { name: "Delete Done last week" }));
      act(() => void vi.advanceTimersByTime(180));
      const confirm = screen.getByRole("button", { name: /Confirm deleting/ });
      fireEvent.click(confirm);

      expect(onDeleteTask).not.toHaveBeenCalled();
      // An ignored press must not look like a delete that happened.
      expect(confirm.getAttribute("aria-disabled")).toBe("true");
      expect(confirm.className).toContain("arming");

      // Focus is already on the button, and a screen reader does not re-announce
      // a focused element changing state, so readiness has to be said aloud.
      expect(screen.getByRole("status").textContent).toBe("");

      act(() => void vi.advanceTimersByTime(500));
      expect(confirm.getAttribute("aria-disabled")).toBe("false");
      expect(confirm.className).not.toContain("arming");
      expect(screen.getByRole("status").textContent).toContain(
        "Press again to delete Done last week",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("stands down when the confirmation is left alone", () => {
    const { onDeleteTask } = renderSidebar([
      task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
    ]);
    vi.useFakeTimers();
    try {
      fireEvent.click(screen.getByRole("button", { name: "Delete Done last week" }));
      // Losing focus is not a cancel: WebKit blurs a button as it is pressed.
      fireEvent.blur(screen.getByRole("button", { name: /Confirm deleting/ }));
      expect(screen.getByRole("button", { name: /Confirm deleting/ })).toBeTruthy();
      act(() => void vi.advanceTimersByTime(4_000));
    } finally {
      vi.useRealTimers();
    }

    expect(screen.queryByRole("button", { name: /Confirm deleting/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Delete Done last week" })).toBeTruthy();
    expect(onDeleteTask).not.toHaveBeenCalled();
  });

  it("cancels the confirmation on Escape and hands focus back to the row", async () => {
    const { onDeleteTask } = renderSidebar([
      task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Delete Done last week" }));
    const confirm = screen.getByRole("button", { name: /Confirm deleting/ });
    expect(document.activeElement).toBe(confirm);

    const reachedWindow = vi.fn();
    window.addEventListener("keydown", reachedWindow);
    fireEvent.keyDown(confirm, { key: "Escape" });
    window.removeEventListener("keydown", reachedWindow);

    // The window-level handler would have closed the sidebar on the same key.
    expect(reachedWindow).not.toHaveBeenCalled();
    expect(onDeleteTask).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Delete Done last week" }),
      ),
    );
  });

  it("survives a completion time no calendar can hold", () => {
    // Formatting it as a date threw while rendering and blanked the window.
    renderSidebar([task({ id: "c", title: "Odd one", done: true, completedAt: 9e18 })]);
    expect(screen.getByText("Completed earlier (1)")).toBeTruthy();
  });
});
