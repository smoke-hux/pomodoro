// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  render(
    <TaskSidebar
      tasks={tasks}
      interruptions={[]}
      notifications={[]}
      captureEnabled={false}
      captureStatus={{ state: "off", detail: "" }}
      activeTaskId={null}
      addRequest={0}
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
    />,
  );
  return { onUpdateTask, onDeleteTask };
}

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

describe("completed tasks", () => {
  it("lists under Completed today only what was completed today", () => {
    renderSidebar([
      task({ id: "a", title: "Done this morning", done: true, completedAt: NOW - 3_600_000 }),
      task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
    ]);

    expect(screen.getByText("Completed today (1)")).toBeTruthy();
    expect(screen.getByText("Completed earlier (1)")).toBeTruthy();
  });

  it("can be deleted, so finished work does not pile up for good", () => {
    const { onDeleteTask } = renderSidebar([
      task({ id: "b", title: "Done last week", done: true, completedAt: NOW - 6 * DAY }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Delete Done last week" }));
    expect(onDeleteTask).toHaveBeenCalledWith("b");
  });
});
