import { useEffect, useRef, useState } from "react";

interface InterruptionDialogProps {
  open: boolean;
  onClose: () => void;
  onSave: (text: string, category: "internal" | "external") => Promise<void>;
}

export function InterruptionDialog({ open, onClose, onSave }: InterruptionDialogProps) {
  const [text, setText] = useState("");
  const [category, setCategory] = useState<"internal" | "external">("internal");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setText("");
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  if (!open) return null;

  const save = async () => {
    if (!text.trim()) return;
    await onSave(text.trim(), category);
    onClose();
  };

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="dialog capture-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="capture-title"
        onMouseDown={(event) => event.stopPropagation()}
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
            <button className="small-primary" type="submit" disabled={!text.trim()}>
              Save to inbox
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
