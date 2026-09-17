// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { InterruptionDialog } from "./InterruptionDialog";

afterEach(cleanup);

type Save = (text: string, category: "internal" | "external") => Promise<boolean>;

function renderDialog(onSave: Save, onClose = vi.fn()) {
  render(<InterruptionDialog open onClose={onClose} onSave={onSave} />);
  return { onClose, note: screen.getByLabelText(/Note it/) as HTMLInputElement };
}

describe("capturing an interruption", () => {
  it("closes once the note has been saved", async () => {
    const onSave = vi.fn<Save>(async () => true);
    const { onClose, note } = renderDialog(onSave);
    fireEvent.change(note, { target: { value: "  Email Jordan " } });
    fireEvent.click(screen.getByRole("button", { name: "Save to inbox" }));

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith("Email Jordan", "internal");
  });

  it("keeps the dialog and the note when the save is refused", async () => {
    // It used to close whatever happened, taking the only copy of the note with it.
    const onSave = vi.fn<Save>(async () => false);
    const { onClose, note } = renderDialog(onSave);
    fireEvent.change(note, { target: { value: "Email Jordan" } });
    fireEvent.click(screen.getByRole("button", { name: "Save to inbox" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "Save to inbox" }) as HTMLButtonElement).disabled).toBe(false),
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(note.value).toBe("Email Jordan");
  });

  it("files the note once when Enter is pressed twice before the save returns", async () => {
    let finish: (saved: boolean) => void = () => {};
    const onSave = vi.fn<Save>(() => new Promise((resolve) => (finish = resolve)));
    const { onClose, note } = renderDialog(onSave);
    fireEvent.change(note, { target: { value: "Email Jordan" } });

    const form = note.closest("form")!;
    fireEvent.submit(form);
    fireEvent.submit(form);
    expect(onSave).toHaveBeenCalledTimes(1);

    finish(true);
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
