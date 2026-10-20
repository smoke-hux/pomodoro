// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

  it("cancels the previous preview before preparing a different file", async () => {
    const next = { ...preview, token: "replacement-file", fileName: "replacement.json" };
    vi.mocked(api.previewImport).mockResolvedValueOnce(preview).mockResolvedValueOnce(next);
    let finishCancel!: () => void;
    vi.mocked(api.cancelImport).mockImplementationOnce(
      () => new Promise((resolve) => (finishCancel = resolve)),
    );
    const { unmount } = render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));

    expect(api.cancelImport).toHaveBeenCalledWith(preview.token);
    expect(api.previewImport).toHaveBeenCalledTimes(1);
    await act(async () => finishCancel());
    await screen.findByRole("heading", { name: "Import replacement.json" });
    unmount();

    expect(api.cancelImport).toHaveBeenCalledTimes(2);
    expect(api.cancelImport).toHaveBeenLastCalledWith(next.token);
  });

  it("discards the old preview when a replacement file picker is cancelled", async () => {
    vi.mocked(api.previewImport).mockResolvedValueOnce(preview).mockResolvedValueOnce(null);
    const { unmount } = render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));

    await waitFor(() => expect(api.previewImport).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(screen.queryByRole("button", { name: "Import and replace" })).toBeNull();
    unmount();
    expect(api.cancelImport).toHaveBeenCalledOnce();
    expect(api.cancelImport).toHaveBeenCalledWith(preview.token);
  });

  it("discards the old preview when reading a replacement file fails", async () => {
    vi.mocked(api.previewImport)
      .mockResolvedValueOnce(preview)
      .mockRejectedValueOnce("This file is not a Pomodoro backup.");
    render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));

    expect((await screen.findByRole("alert")).textContent).toContain("not a Pomodoro backup");
    expect(screen.queryByRole("heading", { name: "Import backup.json" })).toBeNull();
    expect(api.cancelImport).toHaveBeenCalledWith(preview.token);
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
    await waitFor(() => expect(api.previewImport).toHaveBeenCalledOnce());
    unmount();
    finish(preview);
    await waitFor(() => expect(api.cancelImport).toHaveBeenCalledOnce());
    expect(api.cancelImport).toHaveBeenCalledWith(preview.token);
  });

  it("releases the busy guard before reporting a completed import", async () => {
    vi.mocked(api.previewImport).mockResolvedValue(preview);
    vi.mocked(api.confirmImport).mockResolvedValue();
    const onBusyChange = vi.fn();
    const onImported = vi.fn(() => {
      expect(onBusyChange).toHaveBeenLastCalledWith(false);
    });
    render(<DataSettings onImported={onImported} onBusyChange={onBusyChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(screen.getByRole("button", { name: "Import and replace" }));

    await waitFor(() => expect(onImported).toHaveBeenCalledOnce());
  });

  it("does not cancel an in-flight import if the component is forcibly unmounted", async () => {
    vi.mocked(api.previewImport).mockResolvedValue(preview);
    let finish!: () => void;
    vi.mocked(api.confirmImport).mockImplementation(
      () => new Promise((resolve) => (finish = resolve)),
    );
    const onImported = vi.fn();
    const { unmount } = render(<DataSettings onImported={onImported} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(screen.getByRole("button", { name: "Import and replace" }));
    unmount();

    expect(api.cancelImport).not.toHaveBeenCalled();
    await act(async () => finish());
    expect(api.cancelImport).not.toHaveBeenCalled();
    expect(onImported).not.toHaveBeenCalled();
  });

  it("cleans up a failed import after forced unmount once confirmation settles", async () => {
    vi.mocked(api.previewImport).mockResolvedValue(preview);
    let fail!: (reason: string) => void;
    vi.mocked(api.confirmImport).mockImplementation(
      () => new Promise((_, reject) => (fail = reject)),
    );
    const { unmount } = render(<DataSettings onImported={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Choose import" }));
    await screen.findByRole("heading", { name: "Import backup.json" });
    fireEvent.click(screen.getByLabelText("Replace my data with this file"));
    fireEvent.click(screen.getByRole("button", { name: "Import and replace" }));
    unmount();

    expect(api.cancelImport).not.toHaveBeenCalled();
    await act(async () => fail("The backup could not be saved."));
    expect(api.cancelImport).toHaveBeenCalledOnce();
    expect(api.cancelImport).toHaveBeenCalledWith(preview.token);
  });
});
