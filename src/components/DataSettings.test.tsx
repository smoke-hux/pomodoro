// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DataSettings } from "./DataSettings";
import { api } from "../lib/api";

vi.mock("../lib/api", () => ({
  api: {
    exportData: vi.fn(),
    exportSessionsCsv: vi.fn(),
    previewImport: vi.fn(),
    confirmImport: vi.fn(),
    cancelImport: vi.fn(),
    openDataLocation: vi.fn(),
  },
}));

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  vi.resetAllMocks();
  vi.mocked(api.cancelImport).mockResolvedValue();
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

const preview = {
  token: "chosen-file",
  fileName: "backup.json",
  tasks: 3,
  sessions: 12,
  interruptions: 2,
  notifications: 4,
};

describe("data transfer", () => {
  it("requires a preview and explicit confirmation before replacing data", async () => {
    vi.mocked(api.previewImport).mockResolvedValue(preview);
    vi.mocked(api.confirmImport).mockResolvedValue();
    const onImported = vi.fn();
    render(<DataSettings onImported={onImported} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    expect(screen.getByText(/3 tasks · 12 sessions/)).toBeTruthy();
    const replace = screen.getByRole("button", { name: "Import and replace" }) as HTMLButtonElement;
    expect(replace.disabled).toBe(true);
    expect(api.confirmImport).not.toHaveBeenCalled();
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(replace);
    await waitFor(() => expect(onImported).toHaveBeenCalledOnce());
    expect(api.confirmImport).toHaveBeenCalledWith("chosen-file");
  });

  it("retains the preview and reports failed imports without closing", async () => {
    vi.mocked(api.previewImport).mockResolvedValue(preview);
    vi.mocked(api.confirmImport).mockRejectedValue("The backup could not be saved.");
    const onImported = vi.fn();
    render(<DataSettings onImported={onImported} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(screen.getByRole("button", { name: "Import and replace" }));
    expect((await screen.findByRole("alert")).textContent).toContain("backup could not be saved");
    expect(onImported).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Import backup.json" })).toBeTruthy();
  });

  it("cancels the prepared import when its preview is dismissed", async () => {
    vi.mocked(api.previewImport).mockResolvedValue(preview);
    render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel import" }));
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Import backup.json" })).toBeNull(),
    );
    expect(api.cancelImport).toHaveBeenCalledOnce();
    expect(api.confirmImport).not.toHaveBeenCalled();
  });

  it("does not report an export as saved when the file dialog is cancelled", async () => {
    vi.mocked(api.exportData).mockResolvedValue(null);
    render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Export data" }));
    await waitFor(() => expect(api.exportData).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("discards a file selection that completes after the dialog closes", async () => {
    let finish!: (value: typeof preview) => void;
    vi.mocked(api.previewImport).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { unmount } = render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    unmount();
    finish(preview);
    await waitFor(() => expect(api.cancelImport).toHaveBeenCalledOnce());
    expect(api.cancelImport).toHaveBeenCalledWith(preview.token);
  });
});
