import { useEffect, useRef, useState } from "react";
import {
  Check,
  CheckCircle2,
  Circle,
  MoreHorizontal,
  Plus,
  Trash2,
} from "lucide-react";
import type { DesktopNotification, FocusTask, Interruption } from "../types";
import { NotificationInbox } from "./NotificationInbox";

interface TaskSidebarProps {
  tasks: FocusTask[];
  interruptions: Interruption[];
  notifications: DesktopNotification[];
  captureEnabled: boolean;
  activeTaskId: string | null;
  addRequest: number;
  selectionLocked: boolean;
  onSelectTask: (id: string) => void;
  onAddTask: (title: string, estimate: number) => Promise<void>;
  onToggleTask: (id: string) => void;
  onDeleteTask: (id: string) => void;
  onOpenCapture: () => void;
  onHandleInterruption: (id: string, handled: boolean) => void;
  onConvertInterruption: (id: string) => void;
  onDeleteInterruption: (id: string) => void;
  onTriageNotification: (id: string, triaged: boolean) => void;
  onConvertNotification: (id: string) => void;
  onDeleteNotification: (id: string) => void;
  onOpenSettings: () => void;
}

export function TaskSidebar({
  tasks,
  interruptions,
  notifications,
  captureEnabled,
  activeTaskId,
  addRequest,
  selectionLocked,
  onSelectTask,
  onAddTask,
  onToggleTask,
  onDeleteTask,
  onOpenCapture,
  onHandleInterruption,
  onConvertInterruption,
  onDeleteInterruption,
  onTriageNotification,
  onConvertNotification,
  onDeleteNotification,
  onOpenSettings,
}: TaskSidebarProps) {
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const [estimate, setEstimate] = useState(1);
  const inputRef = useRef<HTMLInputElement>(null);
  const openTasks = tasks.filter((task) => !task.done);
  const completedTasks = tasks.filter((task) => task.done);
  const openInterruptions = interruptions.filter((item) => !item.handled);

  useEffect(() => {
    if (adding) inputRef.current?.focus();
  }, [adding]);

  // Ctrl+N raises this counter. Reacting to it keeps the shortcut owned by the
  // component that holds the composer, rather than reaching across the DOM for
  // the button and synthesising a click.
  useEffect(() => {
    if (addRequest > 0) setAdding(true);
  }, [addRequest]);

  const submit = async () => {
    const cleaned = title.trim();
    if (!cleaned) return;
    await onAddTask(cleaned, estimate);
    setTitle("");
    setEstimate(1);
    setAdding(false);
  };

  // A section with nothing filed in it should not hold a third of the sidebar
  // open. When capture is quiet the row collapses to its heading and one line.
  const capturesQuiet = notifications.length === 0;

  return (
    <aside
      className={`sidebar ${capturesQuiet ? "captures-quiet" : ""}`}
      aria-label="Tasks, interruptions, and captured notifications"
    >
      <section className="sidebar-section task-section" aria-labelledby="tasks-heading">
        <div className="section-bar">
          <h2 id="tasks-heading">Today</h2>
          <button
            className="icon-button"
            type="button"
            onClick={() => setAdding(true)}
            aria-label="Add a task"
            title="Add task (Ctrl+N)"
          >
            <Plus aria-hidden="true" size={18} />
          </button>
        </div>

        {adding && (
          <form
            className="task-form"
            onSubmit={(event) => {
              event.preventDefault();
              void submit();
            }}
          >
            <label htmlFor="new-task-title">Task</label>
            <input
              id="new-task-title"
              ref={inputRef}
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="What will you focus on?"
              maxLength={160}
            />
            <div className="task-form-row">
              <label htmlFor="new-task-estimate">Estimate</label>
              <div className="estimate-stepper">
                <button
                  type="button"
                  onClick={() => setEstimate((value) => Math.max(1, value - 1))}
                  aria-label="Decrease estimate"
                >
                  −
                </button>
                <output htmlFor="new-task-estimate">{estimate}</output>
                <input
                  id="new-task-estimate"
                  className="visually-hidden"
                  type="number"
                  min={1}
                  max={16}
                  value={estimate}
                  onChange={(event) =>
                    setEstimate(
                      Math.min(16, Math.max(1, Number(event.target.value) || 1)),
                    )
                  }
                />
                <button
                  type="button"
                  onClick={() => setEstimate((value) => Math.min(16, value + 1))}
                  aria-label="Increase estimate"
                >
                  +
                </button>
              </div>
              <div className="form-actions">
                <button
                  className="text-button"
                  type="button"
                  onClick={() => setAdding(false)}
                >
                  Cancel
                </button>
                <button className="small-primary" type="submit" disabled={!title.trim()}>
                  Add
                </button>
              </div>
            </div>
            {estimate > 4 && (
              <p className="form-hint">
                Consider splitting work above four focus sessions.
              </p>
            )}
          </form>
        )}

        <div className="task-list" role="list" aria-label="Open tasks">
          {openTasks.length === 0 && !adding ? (
            <button className="empty-task" type="button" onClick={() => setAdding(true)}>
              <Plus aria-hidden="true" size={18} />
              Add your first task
            </button>
          ) : (
            openTasks.map((task) => (
              <div
                className={`task-row ${task.id === activeTaskId ? "selected" : ""}`}
                key={task.id}
                role="listitem"
              >
                <button
                  className="task-check"
                  type="button"
                  onClick={() => onToggleTask(task.id)}
                  aria-label={`Mark ${task.title} complete`}
                >
                  <Circle aria-hidden="true" size={17} />
                </button>
                <button
                  className="task-select"
                  type="button"
                  onClick={() => onSelectTask(task.id)}
                  disabled={selectionLocked}
                  aria-pressed={task.id === activeTaskId}
                  aria-label={`${task.title}, ${task.completedPomodoros} of ${task.estimate} sessions`}
                  title={selectionLocked ? "Finish or reset the current focus before switching tasks" : "Select for focus"}
                >
                  <span className="task-title">{task.title}</span>
                  <span className="task-count" aria-hidden="true">
                    {task.completedPomodoros} / {task.estimate}
                  </span>
                </button>
                <details className="row-menu">
                  <summary aria-label={`More actions for ${task.title}`}>
                    <MoreHorizontal aria-hidden="true" size={17} />
                  </summary>
                  <div className="menu-popover">
                    <button type="button" onClick={() => onDeleteTask(task.id)}>
                      <Trash2 aria-hidden="true" size={15} /> Delete
                    </button>
                  </div>
                </details>
              </div>
            ))
          )}
        </div>

        {completedTasks.length > 0 && (
          <details className="completed-group">
            <summary>Completed today ({completedTasks.length})</summary>
            {completedTasks.map((task) => (
              <div className="task-row completed" key={task.id}>
                <button
                  className="task-check"
                  type="button"
                  onClick={() => onToggleTask(task.id)}
                  aria-label={`Reopen ${task.title}`}
                >
                  <CheckCircle2 aria-hidden="true" size={17} />
                </button>
                <span className="completed-title">{task.title}</span>
                <span className="task-count">
                  {task.completedPomodoros} / {task.estimate}
                </span>
              </div>
            ))}
          </details>
        )}
      </section>

      <section className="sidebar-section inbox-section" aria-labelledby="inbox-heading">
        <div className="section-bar">
          <h2 id="inbox-heading">Interruption inbox</h2>
          <button
            className="icon-button"
            type="button"
            onClick={onOpenCapture}
            aria-label="Capture an interruption"
            title="Capture interruption (Ctrl+I)"
          >
            <Plus aria-hidden="true" size={18} />
          </button>
        </div>
        <div className="inbox-list">
          {openInterruptions.length === 0 ? (
            <p className="empty-copy">Distractions you note during focus stay here.</p>
          ) : (
            openInterruptions.map((item) => (
              <div className="inbox-row" key={item.id}>
                <button
                  className="inbox-text"
                  type="button"
                  onClick={() => onHandleInterruption(item.id, true)}
                  title="Mark handled"
                >
                  <span>{item.text}</span>
                  <time dateTime={new Date(item.capturedAt).toISOString()}>
                    {new Date(item.capturedAt).toLocaleTimeString([], {
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </time>
                </button>
                <details className="row-menu">
                  <summary aria-label={`Actions for ${item.text}`}>
                    <MoreHorizontal aria-hidden="true" size={17} />
                  </summary>
                  <div className="menu-popover inbox-menu">
                    <button type="button" onClick={() => onConvertInterruption(item.id)}>
                      <Plus aria-hidden="true" size={15} /> Turn into task
                    </button>
                    <button
                      type="button"
                      onClick={() => onHandleInterruption(item.id, true)}
                    >
                      <Check aria-hidden="true" size={15} /> Mark handled
                    </button>
                    <button type="button" onClick={() => onDeleteInterruption(item.id)}>
                      <Trash2 aria-hidden="true" size={15} /> Delete
                    </button>
                  </div>
                </details>
              </div>
            ))
          )}
        </div>
      </section>

      <NotificationInbox
        notifications={notifications}
        captureEnabled={captureEnabled}
        onTriage={onTriageNotification}
        onConvert={onConvertNotification}
        onDelete={onDeleteNotification}
        onOpenSettings={onOpenSettings}
      />
    </aside>
  );
}
