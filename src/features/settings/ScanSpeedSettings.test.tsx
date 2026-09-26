// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ScanSpeedSettings } from "./ScanSpeedSettings";

const { getScanSettings, setScanProfile } = vi.hoisted(() => ({
  getScanSettings: vi.fn(),
  setScanProfile: vi.fn(),
}));

vi.mock("./api/scanSettings", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api/scanSettings")>()),
  getScanSettings,
  setScanProfile,
}));

describe("ScanSpeedSettings", () => {
  afterEach(() => {
    cleanup();
    getScanSettings.mockReset();
    setScanProfile.mockReset();
  });

  it("loads the saved profile and saves only an explicit change", async () => {
    getScanSettings.mockResolvedValue({ schemaVersion: 1, profile: "auto" });
    setScanProfile.mockResolvedValue({ schemaVersion: 1, profile: "hdd" });

    render(<ScanSpeedSettings />);

    const select = await screen.findByRole("combobox", { name: /^Drive type/ });
    const save = screen.getByRole("button", { name: "Save scan settings" });
    expect((select as HTMLSelectElement).value).toBe("auto");
    expect(screen.getByText(/only build-artifact discovery/).textContent).toContain("one folder at a time");
    expect((save as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(select, { target: { value: "hdd" } });
    expect(screen.getByText(/standard number of folders at once/).textContent).toContain("spinning drives");
    fireEvent.click(save);

    await waitFor(() => expect(setScanProfile).toHaveBeenCalledWith("hdd"));
    expect((await screen.findByRole("status")).textContent).toBe("Scan settings saved.");
    expect((save as HTMLButtonElement).disabled).toBe(true);
  });

  it("offers retry when loading fails", async () => {
    getScanSettings.mockRejectedValueOnce(new Error("offline"));
    getScanSettings.mockResolvedValueOnce({ schemaVersion: 1, profile: "ssd" });

    render(<ScanSpeedSettings />);

    fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
    const select = await screen.findByRole("combobox", { name: /^Drive type/ });
    expect((select as HTMLSelectElement).value).toBe("ssd");
  });

  it("keeps the unsaved change and reports a save failure", async () => {
    getScanSettings.mockResolvedValue({ schemaVersion: 1, profile: "auto" });
    setScanProfile.mockRejectedValue(new Error("denied"));

    render(<ScanSpeedSettings />);

    const select = await screen.findByRole("combobox", { name: /^Drive type/ });
    fireEvent.change(select, { target: { value: "ssd" } });
    fireEvent.click(screen.getByRole("button", { name: "Save scan settings" }));

    expect((await screen.findByRole("alert")).textContent).toContain("could not be saved");
    expect((screen.getByRole("combobox", { name: /^Drive type/ }) as HTMLSelectElement).value).toBe("ssd");
  });
});
