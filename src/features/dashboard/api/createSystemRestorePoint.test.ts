import { afterEach, describe, expect, it, vi } from "vitest";
import { createSystemRestorePoint } from "./createSystemRestorePoint";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("createSystemRestorePoint", () => {
  afterEach(() => invoke.mockReset());

  it("invokes the bounded Tauri command with nested typed input", async () => {
    invoke.mockResolvedValue({ sequenceNumber: 42 });

    await expect(
      createSystemRestorePoint({ description: "Before cleanup" }),
    ).resolves.toEqual({ sequenceNumber: 42 });
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("create_system_restore_point", {
      input: { description: "Before cleanup" },
    });
  });
});
