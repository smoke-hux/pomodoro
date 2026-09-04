import { useEffect, useMemo, useState } from "react";
import {
  Check,
  CheckCircle2,
  Circle,
  MoreHorizontal,
  Plus,
  SlidersHorizontal,
  Trash2,
  Undo2,
} from "lucide-react";
import type { DesktopNotification } from "../types";
import { formatRelativeTime } from "../lib/metrics";

interface NotificationInboxProps {
  notifications: DesktopNotification[];
  captureEnabled: boolean;
  onTriage: (id: string, triaged: boolean) => void;
  onConvert: (id: string) => void;
  onDelete: (id: string) => void;
  onOpenSettings: () => void;
}

// Urgency is a number on the wire. It is never shown as one, and never as
// colour alone — the dot always travels with its word.
const URGENCY_WORD = ["Low", "Normal", "Urgent"] as const;
const URGENCY_CLASS = ["low", "normal", "urgent"] as const;

function urgencyIndex(urgency: number): 0 | 1 | 2 {
  if (urgency >= 2) return 2;
  if (urgency <= 0) return 0;
  return 1;
}

function absoluteTime(timestamp: number): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function isoTime(timestamp: number): string | undefined {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

function NotificationRow({
  item,
  now,
  confirmingDelete,
  onRequestDelete,
  onCancelDelete,
  onTriage,
  onConvert,
  onDelete,
}: {
  item: DesktopNotification;
  now: number;
  confirmingDelete: boolean;
  onRequestDelete: (id: string) => void;
  onCancelDelete: () => void;
  onTriage: (id: string, triaged: boolean) => void;
  onConvert: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const appName = item.appName.trim() || "Unknown app";
  const summary = item.summary.trim() || "No summary";
  const index = urgencyIndex(item.urgency);
  const word = URGENCY_WORD[index];

  return (
    <div
      className={`notice-row ${item.triaged ? "triaged" : ""}`}
      role="listitem"
    >
      <button
        className="notice-check"
        type="button"
        onClick={() => onTriage(item.id, !item.triaged)}
        aria-pressed={item.triaged}
        aria-label={
          item.triaged
            ? `Move ${appName}: ${summary} back to pending`
            : `Mark ${appName}: ${summary} triaged`
        }
        title={item.triaged ? "Move back to pending" : "Mark triaged"}
      >
        {item.triaged ? (
          <CheckCircle2 aria-hidden="true" size={17} />
        ) : (
          <Circle aria-hidden="true" size={17} />
        )}
      </button>

      <div className="notice-content">
        <p className="notice-meta">
          <span className="notice-app">{appName}</span>
          <span className={`notice-urgency ${URGENCY_CLASS[index]}`}>
            <i aria-hidden="true" />
            {word}
          </span>
          {item.duringFocus && <span className="notice-during">During focus</span>}
          <time dateTime={isoTime(item.receivedAt)} title={absoluteTime(item.receivedAt)}>
            {formatRelativeTime(item.receivedAt, now)}
          </time>
        </p>
        <p className="notice-summary">{summary}</p>
        {item.body.trim() !== "" && <p className="notice-body">{item.body}</p>}
      </div>

      <details
        className="row-menu"
        onToggle={(event) => {
          if (!event.currentTarget.open) onCancelDelete();
        }}
      >
        <summary aria-label={`Actions for ${appName}: ${summary}`}>
          <MoreHorizontal aria-hidden="true" size={17} />
        </summary>
        {confirmingDelete ? (
          <div className="menu-popover notice-menu confirming">
            <p className="menu-note">
              Removes this captured copy, including its message text. Any task
              already made from it stays.
            </p>
            <button type="button" onClick={onCancelDelete}>
              Keep it
            </button>
            <button type="button" onClick={() => onDelete(item.id)}>
              Delete
            </button>
          </div>
        ) : (
          <div className="menu-popover notice-menu">
            <button
              type="button"
              // The task is named from the summary, so a notification without
              // one has nothing to become.
              disabled={item.summary.trim() === ""}
              title={
                item.summary.trim() === ""
                  ? "This notification has no summary to name a task"
                  : undefined
              }
              onClick={() => onConvert(item.id)}
            >
              <Plus aria-hidden="true" size={15} /> Turn into task
            </button>
            <button type="button" onClick={() => onTriage(item.id, !item.triaged)}>
              {item.triaged ? (
                <>
                  <Undo2 aria-hidden="true" size={15} /> Move back to pending
                </>
              ) : (
                <>
                  <Check aria-hidden="true" size={15} /> Mark triaged
                </>
              )}
            </button>
            <button type="button" onClick={() => onRequestDelete(item.id)}>
              <Trash2 aria-hidden="true" size={15} /> Delete
            </button>
          </div>
        )}
      </details>
    </div>
  );
}

export function NotificationInbox({
  notifications,
  captureEnabled,
  onTriage,
  onConvert,
  onDelete,
  onOpenSettings,
}: NotificationInboxProps) {
  const [now, setNow] = useState(() => Date.now());
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const { pending, triaged } = useMemo(() => {
    const newestFirst = [...notifications].sort(
      (a, b) => b.receivedAt - a.receivedAt,
    );
    return {
      pending: newestFirst.filter((item) => !item.triaged),
      triaged: newestFirst.filter((item) => item.triaged),
    };
  }, [notifications]);

  const hasAny = notifications.length > 0;

  // Relative labels drift once the timer stops polling. A half-minute tick is
  // enough to keep "12m ago" honest without repainting the sidebar constantly.
  useEffect(() => {
    if (!hasAny) return;
    setNow(Date.now());
    const tick = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(tick);
  }, [hasAny]);

  // A row can disappear underneath an open confirmation, from this window or
  // from a filter change. Drop the pending confirmation rather than stranding it.
  useEffect(() => {
    if (confirmDeleteId && !notifications.some((item) => item.id === confirmDeleteId)) {
      setConfirmDeleteId(null);
    }
  }, [confirmDeleteId, notifications]);

  const rowProps = {
    now,
    onRequestDelete: setConfirmDeleteId,
    onCancelDelete: () => setConfirmDeleteId(null),
    onTriage,
    onConvert,
    onDelete,
  };

  return (
    <section
      className="sidebar-section notice-section"
      aria-labelledby="notifications-heading"
    >
      <div className="section-bar">
        <h2 id="notifications-heading">System notifications</h2>
        {pending.length > 0 && (
          <span className="section-count" aria-hidden="true">
            {pending.length}
          </span>
        )}
        <button
          className="icon-button"
          type="button"
          onClick={onOpenSettings}
          aria-label="Notification capture settings"
          title="Notification capture settings"
        >
          <SlidersHorizontal aria-hidden="true" size={18} />
        </button>
      </div>

      <div
        className="notice-list"
        role={pending.length > 0 ? "list" : undefined}
        aria-label={pending.length > 0 ? "Captured notifications" : undefined}
      >
        {pending.length === 0 ? (
          captureEnabled ? (
            <p className="empty-copy">
              Nothing captured yet. Notifications from other apps will be filed
              here for review after focus.
            </p>
          ) : (
            <p className="empty-copy">
              Capture is off. Turn it on in settings to file notifications from
              other apps here instead of reading them mid-session.
            </p>
          )
        ) : (
          pending.map((item) => (
            <NotificationRow
              key={item.id}
              item={item}
              confirmingDelete={confirmDeleteId === item.id}
              {...rowProps}
            />
          ))
        )}
      </div>

      {triaged.length > 0 && (
        <details className="completed-group notice-group">
          <summary>Triaged ({triaged.length})</summary>
          <div role="list" aria-label="Triaged notifications">
            {triaged.map((item) => (
              <NotificationRow
                key={item.id}
                item={item}
                confirmingDelete={confirmDeleteId === item.id}
                {...rowProps}
              />
            ))}
          </div>
        </details>
      )}
    </section>
  );
}
