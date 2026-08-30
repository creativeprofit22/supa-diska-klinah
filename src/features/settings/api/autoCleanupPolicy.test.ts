import { afterEach, describe, expect, expectTypeOf, it, vi } from "vitest";
import {
  getAutoCleanupPolicy,
  setAutoCleanupPolicy,
  type AutoCleanupPolicy,
} from "./autoCleanupPolicy";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const policy: AutoCleanupPolicy = {
  schemaVersion: 1,
  enabled: true,
  graceDays: 14,
};

describe("autoCleanupPolicy", () => {
  afterEach(() => invoke.mockReset());

  it("gets the typed policy through the exact Tauri command", async () => {
    invoke.mockResolvedValue(policy);

    const result = getAutoCleanupPolicy();

    expectTypeOf(result).toEqualTypeOf<Promise<AutoCleanupPolicy>>();
    await expect(result).resolves.toEqual(policy);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("get_auto_cleanup_policy");
  });

  it("sets the policy with the Rust command's camelCase payload", async () => {
    invoke.mockResolvedValue(policy);

    const result = setAutoCleanupPolicy(true, 14);

    expectTypeOf(result).toEqualTypeOf<Promise<AutoCleanupPolicy>>();
    await expect(result).resolves.toEqual(policy);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("set_auto_cleanup_policy", {
      enabled: true,
      graceDays: 14,
    });
  });

  it("preserves bounded Rust command errors", async () => {
    const error = {
      code: "invalidInput",
      message: "The cleanup request was invalid.",
    };
    invoke.mockRejectedValue(error);

    await expect(setAutoCleanupPolicy(true, 0)).rejects.toBe(error);
    expect(invoke).toHaveBeenCalledOnce();
  });
});
