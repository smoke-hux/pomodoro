import { useEffect, useState } from "react";
import { X } from "lucide-react";
import type { Settings } from "../types";

interface SettingsDialogProps {
  open: boolean;
  settings: Settings;
  onClose: () => void;
  onSave: (settings: Settings) => Promise<void>;
  onClearHistory: () => Promise<void>;
}

function NumberSetting({
  id,
  label,
  value,
  min,
  max,
  suffix = "minutes",
  onChange,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  suffix?: string;
  onChange: (value: number) => void;
}) {
  return (
    <div className="setting-row">
      <label htmlFor={id}>{label}</label>
      <div className="number-setting">
        <input
          id={id}
          type="number"
          value={value}
          min={min}
          max={max}
          onChange={(event) =>
            onChange(Math.min(max, Math.max(min, Number(event.target.value) || min)))
          }
        />
        <span>{suffix}</span>
      </div>
    </div>
  );
}

export function SettingsDialog({
  open,
  settings,
  onClose,
  onSave,
  onClearHistory,
}: SettingsDialogProps) {
  const [draft, setDraft] = useState(settings);
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    if (open) {
      setDraft(settings);
      setConfirmClear(false);
    }
  }, [open, settings]);

  if (!open) return null;

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="dialog settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <h2 id="settings-title">Settings</h2>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close settings">
            <X aria-hidden="true" size={19} />
          </button>
        </header>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void onSave(draft).then(onClose);
          }}
        >
          <div className="settings-scroll">
            <fieldset>
              <legend>Timing</legend>
              <NumberSetting
                id="focus-minutes"
                label="Focus"
                value={draft.focusMinutes}
                min={1}
                max={120}
                onChange={(focusMinutes) => setDraft({ ...draft, focusMinutes })}
              />
              <NumberSetting
                id="short-minutes"
                label="Short break"
                value={draft.shortBreakMinutes}
                min={1}
                max={45}
                onChange={(shortBreakMinutes) => setDraft({ ...draft, shortBreakMinutes })}
              />
              <NumberSetting
                id="long-minutes"
                label="Long break"
                value={draft.longBreakMinutes}
                min={5}
                max={60}
                onChange={(longBreakMinutes) => setDraft({ ...draft, longBreakMinutes })}
              />
              <NumberSetting
                id="round-count"
                label="Rounds before a long break"
                value={draft.roundsBeforeLongBreak}
                min={2}
                max={8}
                suffix="rounds"
                onChange={(roundsBeforeLongBreak) =>
                  setDraft({ ...draft, roundsBeforeLongBreak })
                }
              />
            </fieldset>

            <fieldset>
              <legend>Flow</legend>
              <label className="toggle-row">
                <span>
                  <strong>Start breaks automatically</strong>
                  <small>Move straight into recovery when focus ends.</small>
                </span>
                <input
                  type="checkbox"
                  checked={draft.autoStartBreaks}
                  onChange={(event) =>
                    setDraft({ ...draft, autoStartBreaks: event.target.checked })
                  }
                />
              </label>
              <label className="toggle-row">
                <span>
                  <strong>Start focus automatically</strong>
                  <small>Begin the next selected task when a break ends.</small>
                </span>
                <input
                  type="checkbox"
                  checked={draft.autoStartFocus}
                  onChange={(event) =>
                    setDraft({ ...draft, autoStartFocus: event.target.checked })
                  }
                />
              </label>
            </fieldset>

            <fieldset>
              <legend>Alerts</legend>
              <label className="toggle-row">
                <span>
                  <strong>Desktop notifications</strong>
                  <small>Show an Ubuntu notification at every boundary.</small>
                </span>
                <input
                  type="checkbox"
                  checked={draft.notifications}
                  onChange={(event) =>
                    setDraft({ ...draft, notifications: event.target.checked })
                  }
                />
              </label>
              <label className="toggle-row">
                <span>
                  <strong>Notification sound</strong>
                  <small>Use the desktop’s standard message sound.</small>
                </span>
                <input
                  type="checkbox"
                  checked={draft.sound}
                  onChange={(event) => setDraft({ ...draft, sound: event.target.checked })}
                />
              </label>
            </fieldset>

            <fieldset>
              <legend>Appearance</legend>
              <div className="setting-row">
                <label htmlFor="theme">Theme</label>
                <select
                  id="theme"
                  value={draft.theme}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      theme: event.target.value as Settings["theme"],
                    })
                  }
                >
                  <option value="system">System</option>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
              </div>
            </fieldset>

            <fieldset>
              <legend>Data</legend>
              <div className="data-row">
                <span>
                  <strong>Session history</strong>
                  <small>Tasks stay in place; completed session records are removed.</small>
                </span>
                {confirmClear ? (
                  <span className="clear-confirm">
                    <button className="text-button" type="button" onClick={() => setConfirmClear(false)}>
                      Cancel
                    </button>
                    <button className="danger-button" type="button" onClick={() => void onClearHistory()}>
                      Confirm clear
                    </button>
                  </span>
                ) : (
                  <button className="secondary-control" type="button" onClick={() => setConfirmClear(true)}>
                    Clear history
                  </button>
                )}
              </div>
            </fieldset>

            <fieldset>
              <legend>Keyboard</legend>
              <dl className="shortcut-list">
                <div><dt><kbd>Space</kbd></dt><dd>Start or pause</dd></div>
                <div><dt><kbd>Ctrl I</kbd></dt><dd>Capture interruption</dd></div>
                <div><dt><kbd>Ctrl N</kbd></dt><dd>Add task</dd></div>
                <div><dt><kbd>Ctrl 1–3</kbd></dt><dd>Choose timer mode</dd></div>
                <div><dt><kbd>Ctrl ,</kbd></dt><dd>Open settings</dd></div>
              </dl>
            </fieldset>
          </div>
          <div className="dialog-actions settings-actions">
            <button className="text-button" type="button" onClick={onClose}>Cancel</button>
            <button className="small-primary" type="submit">Save settings</button>
          </div>
        </form>
      </section>
    </div>
  );
}
