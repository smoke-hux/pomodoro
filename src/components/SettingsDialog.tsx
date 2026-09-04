import { useEffect, useState } from "react";
import { X } from "lucide-react";
import type { NotificationFilter, Settings, TimerFace as TimerFaceId } from "../types";
import { TimerFace } from "./TimerFace";

/**
 * Previewed at 15:00 of a 25:00 interval so every face shows a partial state —
 * a full or empty ring tells the user nothing about how the face behaves.
 */
const PREVIEW_DURATION = 25 * 60;
const PREVIEW_REMAINING = 15 * 60;

const TIMER_FACES: { id: TimerFaceId; name: string; note: string }[] = [
  { id: "digits", name: "Digits", note: "Exact to the second" },
  { id: "ring", name: "Ring", note: "Proportion at a glance" },
  { id: "arc", name: "Arc", note: "A half-circle gauge" },
  { id: "analog", name: "Analog", note: "A wind-up kitchen dial" },
  { id: "orbit", name: "Orbit", note: "One dot, one revolution" },
  { id: "bar", name: "Bar", note: "A single depleting line" },
  { id: "blocks", name: "Blocks", note: "Twelve fixed segments" },
  { id: "pips", name: "Pips", note: "One mark per minute" },
  { id: "vessel", name: "Vessel", note: "A quantity draining" },
  { id: "words", name: "Words", note: "No numerals at all" },
];

interface SettingsDialogProps {
  open: boolean;
  settings: Settings;
  notificationCount: number;
  onClose: () => void;
  onSave: (settings: Settings) => Promise<void>;
  onClearHistory: () => Promise<void>;
  onClearNotifications: () => Promise<void>;
}

// A named list of app names, added one at a time and removed by chip. Matching
// on the backend is case-insensitive, so a duplicate that differs only in case
// is silently folded into the existing entry rather than listed twice.
function AppListEditor({
  id,
  label,
  hint,
  items,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  hint: string;
  items: string[];
  disabled: boolean;
  onChange: (items: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  const cleaned = draft.trim();

  const add = () => {
    if (!cleaned) return;
    const known = items.some(
      (item) => item.toLowerCase() === cleaned.toLowerCase(),
    );
    if (!known) onChange([...items, cleaned]);
    setDraft("");
  };

  return (
    <div className={`app-list ${disabled ? "setting-disabled" : ""}`}>
      <label htmlFor={id}>{label}</label>
      <small id={`${id}-hint`}>{hint}</small>
      <div className="app-list-entry">
        <input
          id={id}
          type="text"
          value={draft}
          disabled={disabled}
          maxLength={64}
          placeholder="App name"
          aria-describedby={`${id}-hint`}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            // The dialog is one big form. Enter here means "add", not "save".
            if (event.key === "Enter") {
              event.preventDefault();
              add();
            }
          }}
        />
        <button
          className="secondary-control"
          type="button"
          disabled={disabled || !cleaned}
          onClick={add}
        >
          Add
        </button>
      </div>
      {items.length > 0 && (
        <ul className="app-chips">
          {items.map((item, index) => (
            // Keyed and removed by position: a stored list written before this
            // editor existed may hold two entries that differ only in case, and
            // removing by value would take both.
            <li key={`${index}-${item}`}>
              <span>{item}</span>
              <button
                type="button"
                disabled={disabled}
                aria-label={`Remove ${item} from ${label.toLowerCase()}`}
                onClick={() =>
                  onChange(items.filter((_, position) => position !== index))
                }
              >
                <X aria-hidden="true" size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
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
  notificationCount,
  onClose,
  onSave,
  onClearHistory,
  onClearNotifications,
}: SettingsDialogProps) {
  const [draft, setDraft] = useState(settings);
  const [confirmClear, setConfirmClear] = useState(false);
  const [confirmClearNotifications, setConfirmClearNotifications] = useState(false);

  useEffect(() => {
    if (open) {
      setDraft(settings);
      setConfirmClear(false);
      setConfirmClearNotifications(false);
    }
  }, [open, settings]);

  if (!open) return null;

  const filter = draft.notificationFilter;
  const setFilter = (patch: Partial<NotificationFilter>) =>
    setDraft({ ...draft, notificationFilter: { ...filter, ...patch } });
  const captureOff = !filter.enabled;

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
              <legend>Notification capture</legend>
              <p className="fieldset-note">
                Pomodoro can watch the desktop notification service and file what
                other apps send, so it can be read after the interval instead of
                during it. Captured text stays in Pomodoro’s local data on this
                machine. It is never sent anywhere, and the app makes no network
                requests. Capture stays off until you turn it on here.
              </p>
              <label className="toggle-row">
                <span>
                  <strong>Capture desktop notifications</strong>
                  <small>Off by default. Nothing is watched until this is on.</small>
                </span>
                <input
                  type="checkbox"
                  checked={filter.enabled}
                  onChange={(event) => setFilter({ enabled: event.target.checked })}
                />
              </label>
              <div className={`setting-row ${captureOff ? "setting-disabled" : ""}`}>
                <label htmlFor="min-urgency">Minimum urgency</label>
                <select
                  id="min-urgency"
                  value={String(filter.minUrgency)}
                  disabled={captureOff}
                  onChange={(event) =>
                    setFilter({
                      minUrgency: Number(event.target.value) as
                        NotificationFilter["minUrgency"],
                    })
                  }
                >
                  <option value="0">Low and above</option>
                  <option value="1">Normal and above</option>
                  <option value="2">Urgent only</option>
                </select>
              </div>
              <label className={`toggle-row ${captureOff ? "setting-disabled" : ""}`}>
                <span>
                  <strong>Only during focus</strong>
                  <small>Ignore anything that arrives outside a focus interval.</small>
                </span>
                <input
                  type="checkbox"
                  checked={filter.focusOnly}
                  disabled={captureOff}
                  onChange={(event) => setFilter({ focusOnly: event.target.checked })}
                />
              </label>
              <AppListEditor
                id="muted-apps"
                label="Muted apps"
                hint="Never captured. Muting wins over priority."
                items={filter.mutedApps}
                disabled={captureOff}
                onChange={(mutedApps) => setFilter({ mutedApps })}
              />
              <AppListEditor
                id="priority-apps"
                label="Priority apps"
                hint="Always captured, ignoring minimum urgency and the focus-only rule."
                items={filter.priorityApps}
                disabled={captureOff}
                onChange={(priorityApps) => setFilter({ priorityApps })}
              />
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

              <div className="setting-stack">
                <span className="setting-stack-label" id="timer-face-label">
                  Timer face
                </span>
                <p className="setting-hint">
                  How the remaining time is drawn. Each one trades precision for calm
                  differently.
                </p>
                <div
                  className="face-picker"
                  role="radiogroup"
                  aria-labelledby="timer-face-label"
                >
                  {TIMER_FACES.map((option) => (
                    <button
                      key={option.id}
                      type="button"
                      role="radio"
                      aria-checked={draft.timerFace === option.id}
                      className={`face-option${draft.timerFace === option.id ? " selected" : ""}`}
                      onClick={() => setDraft({ ...draft, timerFace: option.id })}
                    >
                      <span className="face-preview" aria-hidden="true">
                        <TimerFace
                          face={option.id}
                          phase="focus"
                          remainingSeconds={PREVIEW_REMAINING}
                          durationSeconds={PREVIEW_DURATION}
                          phaseLabel="Focus"
                        />
                      </span>
                      <span className="face-name">{option.name}</span>
                      <span className="face-note">{option.note}</span>
                    </button>
                  ))}
                </div>
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
              <div className="data-row">
                <span>
                  <strong>Captured notifications</strong>
                  <small>
                    {notificationCount === 0
                      ? "Nothing is filed right now."
                      : `Removes all ${notificationCount} captured copies, including their message text. Tasks already made from them stay.`}
                  </small>
                </span>
                {confirmClearNotifications ? (
                  <span className="clear-confirm">
                    <button
                      className="text-button"
                      type="button"
                      onClick={() => setConfirmClearNotifications(false)}
                    >
                      Cancel
                    </button>
                    <button
                      className="danger-button"
                      type="button"
                      onClick={() => void onClearNotifications()}
                    >
                      Confirm clear
                    </button>
                  </span>
                ) : (
                  <button
                    className="secondary-control"
                    type="button"
                    disabled={notificationCount === 0}
                    onClick={() => setConfirmClearNotifications(true)}
                  >
                    Clear captured
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
