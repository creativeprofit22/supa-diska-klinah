import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getScanSettings, parseScanSettings, setScanProfile } from "./scanSettings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("scan settings API", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("uses the scan settings commands with only the profile", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ schemaVersion: 1, profile: "auto" });
    vi.mocked(invoke).mockResolvedValueOnce({ schemaVersion: 1, profile: "hdd" });

    await expect(getScanSettings()).resolves.toEqual({ schemaVersion: 1, profile: "auto" });
    await expect(setScanProfile("hdd")).resolves.toEqual({ schemaVersion: 1, profile: "hdd" });

    expect(invoke).toHaveBeenNthCalledWith(1, "get_scan_settings");
    expect(invoke).toHaveBeenNthCalledWith(2, "set_scan_settings", { profile: "hdd" });
  });

  it.each([
    null,
    "ssd",
    { schemaVersion: 2, profile: "ssd" },
    { schemaVersion: 1, profile: "nvme" },
    { schemaVersion: 1 },
  ])("rejects malformed responses: %j", (value) => {
    expect(() => parseScanSettings(value)).toThrow("Invalid scan settings response.");
  });

  it("refuses to send unknown profiles", async () => {
    await expect(setScanProfile("fast" as never)).rejects.toThrow("Unknown scan profile.");
    expect(invoke).not.toHaveBeenCalled();
  });
});
