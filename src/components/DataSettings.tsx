import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { ImportPreview } from "../types";
import "../styles/data-transfer.css";

export function DataSettings({
  onImported,
  onBusyChange,
}: {
  onImported: () => void;
  onBusyChange?: (busy: boolean) => void;
}) {
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const mounted = useRef(true);
  const pending = useRef<string | null>(null);
  const previewHeading = useRef<HTMLHeadingElement>(null);
  const desktop = "__TAURI_INTERNALS__" in window;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (pending.current) void api.cancelImport(pending.current).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (preview) previewHeading.current?.focus();
  }, [preview]);

  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    onBusyChange?.(true);
    setError("");
    setStatus("");
    try {
      await action();
    } catch (reason) {
      if (mounted.current) setError(String(reason));
    } finally {
      onBusyChange?.(false);
      if (mounted.current) setBusy(false);
    }
  }

  async function chooseImport() {
    const next = await api.previewImport();
    if (!mounted.current) {
      if (next) await api.cancelImport(next.token);
      return;
    }
    pending.current = next?.token ?? null;
    setPreview(next);
    setConfirmed(false);
  }

  async function cancelImport() {
    if (pending.current) await api.cancelImport(pending.current);
    pending.current = null;
    setPreview(null);
    setConfirmed(false);
  }

  return (
    <fieldset className="data-transfer" disabled={busy || !desktop}>
      <legend>Backup and transfer</legend>
      {!desktop && <p className="setting-hint">Open the desktop app to manage saved data.</p>}
      <div className="data-row">
        <span>
          <strong>Export your data</strong>
          <small>
            Save tasks, settings and all retained history. The file also contains captured message
            text; keep it private.
          </small>
        </span>
        <button
          type="button"
          className="secondary-control"
          onClick={() =>
            void run(async () => {
              const path = await api.exportData();
              if (path && mounted.current) setStatus(`Data exported to ${path}`);
            })
          }
        >
          Export data
        </button>
      </div>
      <div className="data-row">
        <span>
          <strong>Session spreadsheet</strong>
          <small>Export all retained session records as CSV, with UTC dates.</small>
        </span>
        <button
          type="button"
          className="secondary-control"
          onClick={() =>
            void run(async () => {
              const path = await api.exportSessionsCsv();
              if (path && mounted.current) setStatus(`Sessions exported to ${path}`);
            })
          }
        >
          Export CSV
        </button>
      </div>
      <div className="data-row">
        <span>
          <strong>Restore or move your data</strong>
          <small>
            Preview a Pomodoro JSON file before replacing your data. Reset any active interval
            first.
          </small>
        </span>
        <button type="button" className="secondary-control" onClick={() => void run(chooseImport)}>
          Choose import
        </button>
      </div>
      {preview && (
        <div className="import-preview" aria-labelledby="import-preview-title">
          <h3 id="import-preview-title" ref={previewHeading} tabIndex={-1}>
            Import {preview.fileName}
          </h3>
          <p>
            {preview.tasks} tasks · {preview.sessions} sessions · {preview.interruptions}{" "}
            interruptions · {preview.notifications} notifications
          </p>
          <p>
            This replaces your saved data and settings. A recovery copy is kept in the data folder.
            Imported timers stay paused; notification capture and Do Not Disturb stay off until you
            enable them again.
          </p>
          <label className="import-confirmation">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(event) => setConfirmed(event.target.checked)}
            />
            Replace my data with this file
          </label>
          <div className="import-actions">
            <button type="button" className="text-button" onClick={() => void run(cancelImport)}>
              Cancel import
            </button>
            <button
              type="button"
              className="danger-button"
              disabled={!confirmed}
              onClick={() =>
                void run(async () => {
                  await api.confirmImport(preview.token);
                  pending.current = null;
                  if (mounted.current) onImported();
                })
              }
            >
              Import and replace
            </button>
          </div>
        </div>
      )}
      <div className="data-row">
        <span>
          <strong>Local data and recovery copies</strong>
          <small>
            The five latest backups are kept before import, storage upgrades and clearing history.
            They may contain previously captured messages. You can manage those copies in the data
            folder.
          </small>
        </span>
        <button
          type="button"
          className="secondary-control"
          onClick={() => void run(() => api.openDataLocation())}
        >
          Open data folder
        </button>
      </div>
      {busy && <p role="status">Working…</p>}
      {status && (
        <p role="status" className="transfer-message">
          {status}
        </p>
      )}
      {error && (
        <p role="alert" className="transfer-message">
          {error}
        </p>
      )}
    </fieldset>
  );
}
