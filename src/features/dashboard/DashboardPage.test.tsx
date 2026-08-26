// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DashboardPage } from "./DashboardPage";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

vi.mock("./model/useFoundationStatus", () => ({
  useFoundationStatus: () => ({
    status: { platform: "windows", architecture: "x86_64", adapterReady: true },
    error: null,
    loading: false,
    retry: vi.fn(),
  }),
}));

describe("Dashboard restore-point action", () => {
  afterEach(() => {
    cleanup();
    invoke.mockReset();
  });

  it("requires explicit confirmation before requesting elevation", async () => {
    invoke.mockResolvedValue({ sequenceNumber: 42 });
    render(<DashboardPage />);

    expect(invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Create restore point" }));
    expect(invoke).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Confirm and continue" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
    expect(invoke).toHaveBeenCalledWith("create_system_restore_point", {
      input: { description: "Supa Diska Klinah safety restore point" },
    });
  });

  it("renders the sequence number returned by Windows", async () => {
    invoke.mockResolvedValue({ sequenceNumber: 73 });
    render(<DashboardPage />);

    fireEvent.click(screen.getByRole("button", { name: "Create restore point" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm and continue" }));

    expect((await screen.findByRole("status")).textContent).toContain(
      "Restore point created. Sequence number: 73",
    );
  });

  it("returns to a retryable state when elevation is denied", async () => {
    invoke.mockRejectedValue({ code: "operationUnavailable" });
    render(<DashboardPage />);

    fireEvent.click(screen.getByRole("button", { name: "Create restore point" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm and continue" }));

    const alert = await screen.findByRole("alert");
    expect(screen.queryByText(/Restore point created/)).toBeNull();
    expect(alert.textContent).toBe(
      "Windows did not complete the restore point. This app is still open.",
    );

    const retry = screen.getByRole("button", { name: "Create restore point" });
    expect((retry as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(retry);
    expect(screen.getByRole("button", { name: "Confirm and continue" })).toBeTruthy();
  });
});
