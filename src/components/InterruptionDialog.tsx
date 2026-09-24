import { memo, useEffect, useRef, useState } from "react";
import { useModalFocus } from "../lib/useModalFocus";

interface InterruptionDialogProps {
  open: boolean;
  onClose: () => void;
  /** Resolves to whether the note was saved. A refused one keeps the dialog, and the note, open. */
  onSave: (text: string, category: "internal" | "external") => Promise<boolean>;
}

function InterruptionDialogComponent({ open, onClose, onSave }: InterruptionDialogProps) {
  const [text, setText] = useState("");
  const [category, setCategory] = useState<"internal" | "external">("internal");
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (open) {
      setText("");
      setSaving(false);
    }
  }, [open]);
  useModalFocus(open, dialogRef, inputRef);

  if (!open) return null;

  const save = async () => {
    // `saving` stops Enter pressed twice from filing the same note twice: the
    // dialog stays open, with the text still in it, until the first save returns.
    if (!text.trim() || saving) return;
    setSaving(true);
    const saved = await onSave(text.trim(), category);
    setSaving(false);
    // Closing regardless threw the note away exactly when it had not been
    // kept: a save the backend refused closed the dialog over the only copy.
    if (saved) onClose();
  };

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="dialog capture-dialog"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="capture-title"
      >
        <h2 id="capture-title">What came up?</h2>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <label htmlFor="interruption-text">Note it, then return to focus</label>
          <input
            id="interruption-text"
            ref={inputRef}
            value={text}
            maxLength={200}
            onChange={(event) => setText(event.target.value)}
            placeholder="Email Jordan after this session"
          />
          <fieldset className="category-choice">
            <legend>Source</legend>
            <label>
              <input
                type="radio"
                name="category"
                checked={category === "internal"}
                onChange={() => setCategory("internal")}
              />
              Internal
            </label>
            <label>
              <input
                type="radio"
                name="category"
                checked={category === "external"}
                onChange={() => setCategory("external")}
              />
              External
            </label>
          </fieldset>
          <div className="dialog-actions">
            <button className="text-button" type="button" onClick={onClose}>
              Cancel
            </button>
            <button className="small-primary" type="submit" disabled={!text.trim() || saving}>
              Save to inbox
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}

/**
 * Memoised: the window re-renders every second while the timer runs, and this
 * component has nothing to do with the countdown. Its props are stable
 * callbacks and slices of the snapshot, so it only re-renders when something
 * it shows actually changed.
 */
export const InterruptionDialog = memo(InterruptionDialogComponent);
