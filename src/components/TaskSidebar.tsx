import { memo, useEffect, useRef, useState } from "react";
import {
  Check,
  CheckCircle2,
  Circle,
  MoreHorizontal,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react";
import type {
  CaptureStatus,
  DesktopNotification,
  FocusTask,
  Interruption,
} from "../types";
import { getLocalDateKey } from "../lib/metrics";
import { NotificationInbox } from "./NotificationInbox";

interface TaskSidebarProps {
  tasks: FocusTask[];
  interruptions: Interruption[];
  notifications: DesktopNotification[];
  captureEnabled: boolean;
  captureStatus: CaptureStatus;
  activeTaskId: string | null;
  addRequest: number;
  selectionLocked: boolean;
  /** Today's local date; changes at midnight so "Completed today" rolls over. */
  dayKey: string;
  onSelectTask: (id: string) => void;
  /** Both resolve to whether the change was accepted, so a refused one keeps its form open. */
  onAddTask: (title: string, estimate: number) => Promise<boolean>;
  onUpdateTask: (id: string, title: string, estimate: number) => Promise<boolean>;
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

interface TaskComposerProps {
  /** Prefixes the field ids, so the add and edit forms never share one. */
  idPrefix: string;
  initialTitle?: string;
  initialEstimate?: number;
  submitLabel: string;
  onSubmit: (title: string, estimate: number) => Promise<boolean>;
  onCancel: () => void;
}

/** The title-and-estimate form, used both to add a task and to edit one. */
function TaskComposer({
  idPrefix,
  initialTitle = "",
  initialEstimate = 1,
  submitLabel,
  onSubmit,
  onCancel,
}: TaskComposerProps) {
  const [title, setTitle] = useState(initialTitle);
  const [estimate, setEstimate] = useState(initialEstimate);
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const submit = async () => {
    const cleaned = title.trim();
    if (!cleaned || saving) return;
    setSaving(true);
    // On success the parent unmounts this form; on refusal it stays, with
    // what was typed, under the notice that says why.
    const accepted = await onSubmit(cleaned, estimate);
    if (!accepted) setSaving(false);
  };

  return (
    <form
      className="task-form"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        onCancel();
      }}
    >
      <label htmlFor={`${idPrefix}-title`}>Task</label>
      <input
        id={`${idPrefix}-title`}
        ref={inputRef}
        value={title}
        onChange={(event) => setTitle(event.target.value)}
        placeholder="What will you focus on?"
        maxLength={160}
      />
      <div className="task-form-row">
        <label htmlFor={`${idPrefix}-estimate`}>Estimate</label>
        <div className="estimate-stepper">
          <button
            type="button"
            onClick={() => setEstimate((value) => Math.max(1, value - 1))}
            aria-label="Decrease estimate"
          >
            −
          </button>
          <output htmlFor={`${idPrefix}-estimate`}>{estimate}</output>
          <input
            id={`${idPrefix}-estimate`}
            className="visually-hidden"
            type="number"
            min={1}
            max={16}
            value={estimate}
            onChange={(event) =>
              setEstimate(Math.min(16, Math.max(1, Number(event.target.value) || 1)))
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
          <button className="text-button" type="button" onClick={onCancel}>
            Cancel
          </button>
          <button className="small-primary" type="submit" disabled={!title.trim() || saving}>
            {submitLabel}
          </button>
        </div>
      </div>
      {estimate > 4 && (
        <p className="form-hint">Consider splitting work above four focus sessions.</p>
      )}
    </form>
  );
}

function TaskSidebarComponent({
  tasks,
  interruptions,
  notifications,
  captureEnabled,
  captureStatus,
  activeTaskId,
  addRequest,
  selectionLocked,
  dayKey,
  onSelectTask,
  onAddTask,
  onUpdateTask,
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
  const [editingId, setEditingId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const openTasks = tasks.filter((task) => !task.done);
  // The heading says "today", so it lists today. Tasks finished on an earlier
  // day used to pile up under it for good, with no way to remove them.
  const completedToday = tasks.filter(
    (task) => task.done && task.completedAt !== null && getLocalDateKey(task.completedAt) === dayKey,
  );
  const completedEarlier = tasks.filter((task) => task.done && !completedToday.includes(task));
  const openInterruptions = interruptions.filter((item) => !item.handled);

  // Ctrl+N raises this counter. Reacting to it keeps the shortcut owned by the
  // component that holds the composer, rather than reaching across the DOM for
  // the button and synthesising a click.
  useEffect(() => {
    if (addRequest > 0) setAdding(true);
  }, [addRequest]);

  // A completed row's delete is a single small button beside "reopen", so it
  // asks once, the way Skip does, and stands down by itself if left alone.
  useEffect(() => {
    if (confirmDeleteId === null) return;
    const timeout = window.setTimeout(() => setConfirmDeleteId(null), 4_000);
    return () => window.clearTimeout(timeout);
  }, [confirmDeleteId]);

  // Opening the editor replaces the row, and with it the menu button that had
  // focus. When the editor closes, focus goes back to that row rather than
  // dropping to <body> and restarting Tab from the top of the window.
  const stopEditing = (id: string) => {
    setEditingId(null);
    requestAnimationFrame(() => {
      for (const row of listRef.current?.querySelectorAll<HTMLElement>("[data-task-id]") ?? []) {
        if (row.dataset.taskId === id) row.querySelector<HTMLElement>(".row-menu > summary")?.focus();
      }
    });
  };

  const submitNew = async (title: string, estimate: number) => {
    const accepted = await onAddTask(title, estimate);
    if (accepted) setAdding(false);
    return accepted;
  };

  const completedRow = (task: FocusTask) => (
    <div
      className={`task-row completed${confirmDeleteId === task.id ? " confirming" : ""}`}
      key={task.id}
    >
      <button
        className="task-check"
        type="button"
        onClick={() => onToggleTask(task.id)}
        aria-label={`Reopen ${task.title}`}
      >
        <CheckCircle2 aria-hidden="true" size={17} />
      </button>
      <span className="completed-title">{task.title}</span>
      {confirmDeleteId === task.id ? (
        <button
          className="row-delete confirming"
          type="button"
          onClick={() => {
            setConfirmDeleteId(null);
            onDeleteTask(task.id);
          }}
          // No cancel-on-blur: WebKit does not keep focus on a button being
          // clicked, so the press itself blurred this and withdrew the
          // confirmation before the click could land. The timeout stands in.
          // The button it replaces had focus; without this a keyboard user
          // would be dropped to <body> halfway through deleting.
          autoFocus
          aria-label={`Confirm deleting ${task.title}, including its count of ${task.completedPomodoros} completed sessions`}
          title="Removes the task and its session count. Today's ledger keeps its sessions."
        >
          Delete task?
        </button>
      ) : (
        <>
          <span className="task-count">
            {task.completedPomodoros} / {task.estimate}
          </span>
          <button
            className="row-delete"
            type="button"
            onClick={() => setConfirmDeleteId(task.id)}
            aria-label={`Delete ${task.title}`}
            title="Delete"
          >
            <Trash2 aria-hidden="true" size={15} />
          </button>
        </>
      )}
    </div>
  );

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
          <TaskComposer
            idPrefix="new-task"
            submitLabel="Add"
            onSubmit={submitNew}
            onCancel={() => setAdding(false)}
          />
        )}

        <div className="task-list" role="list" aria-label="Open tasks" ref={listRef}>
          {openTasks.length === 0 && !adding ? (
            <button className="empty-task" type="button" onClick={() => setAdding(true)}>
              <Plus aria-hidden="true" size={18} />
              Add your first task
            </button>
          ) : (
            openTasks.map((task) =>
              task.id === editingId ? (
                <div role="listitem" key={task.id}>
                  <TaskComposer
                    idPrefix={`edit-${task.id}`}
                    initialTitle={task.title}
                    initialEstimate={task.estimate}
                    submitLabel="Save"
                    onSubmit={async (title, estimate) => {
                      const accepted = await onUpdateTask(task.id, title, estimate);
                      if (accepted) stopEditing(task.id);
                      return accepted;
                    }}
                    onCancel={() => stopEditing(task.id)}
                  />
                </div>
              ) : (
                <div
                  className={`task-row ${task.id === activeTaskId ? "selected" : ""}`}
                  key={task.id}
                  role="listitem"
                  data-task-id={task.id}
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
                      <button type="button" onClick={() => setEditingId(task.id)}>
                        <Pencil aria-hidden="true" size={15} /> Edit
                      </button>
                      <button type="button" onClick={() => onDeleteTask(task.id)}>
                        <Trash2 aria-hidden="true" size={15} /> Delete
                      </button>
                    </div>
                  </details>
                </div>
              ),
            )
          )}
        </div>

        {completedToday.length > 0 && (
          <details className="completed-group">
            <summary>Completed today ({completedToday.length})</summary>
            {completedToday.map(completedRow)}
          </details>
        )}
        {completedEarlier.length > 0 && (
          <details className="completed-group">
            <summary>Completed earlier ({completedEarlier.length})</summary>
            {completedEarlier.map(completedRow)}
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
        captureStatus={captureStatus}
        onTriage={onTriageNotification}
        onConvert={onConvertNotification}
        onDelete={onDeleteNotification}
        onOpenSettings={onOpenSettings}
      />
    </aside>
  );
}

/**
 * Memoised: the window re-renders every second while the timer runs, and this
 * component has nothing to do with the countdown. Its props are stable
 * callbacks and slices of the snapshot, so it only re-renders when something
 * it shows actually changed.
 */
export const TaskSidebar = memo(TaskSidebarComponent);
