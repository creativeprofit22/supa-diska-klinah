// @vitest-environment jsdom

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useCreateSystemRestorePoint } from "./useCreateSystemRestorePoint";

const createSystemRestorePoint = vi.hoisted(() => vi.fn());

vi.mock("../api/createSystemRestorePoint", () => ({ createSystemRestorePoint }));

describe("useCreateSystemRestorePoint", () => {
  afterEach(() => {
    cleanup();
    createSystemRestorePoint.mockReset();
  });

  it("reports loading and the returned sequence number", async () => {
    let finish!: (value: { sequenceNumber: number }) => void;
    createSystemRestorePoint.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { result } = renderHook(() => useCreateSystemRestorePoint());
    let request!: Promise<void>;

    act(() => {
      request = result.current.create({ description: "Before cleanup" });
    });
    expect(result.current.loading).toBe(true);

    finish({ sequenceNumber: 73 });
    await act(async () => request);

    expect(result.current.loading).toBe(false);
    expect(result.current.result).toEqual({ sequenceNumber: 73 });
    expect(result.current.error).toBeNull();
  });

  it.each([
    [
      "authorizationCancelled",
      "Administrator approval was cancelled. Try again and approve the Windows prompt.",
    ],
    [
      "helperUnavailable",
      "The privileged helper is unavailable. Repair or reinstall the app, then try again.",
    ],
    [
      "operationTimedOut",
      "System Restore timed out. Check Windows System Protection, then try again.",
    ],
    [
      "invalidRequest",
      "The restore point request expired or was rejected. Try again.",
    ],
    [
      "privilegeFailure",
      "Administrator access was not granted. Try again and approve the Windows prompt.",
    ],
    [
      "systemRestoreFailure",
      "Windows System Restore failed. Check System Protection and available disk space.",
    ],
  ])("provides recovery guidance for %s", async (code, message) => {
    createSystemRestorePoint.mockRejectedValue({ code });
    const { result } = renderHook(() => useCreateSystemRestorePoint());

    await act(async () => {
      await result.current.create({ description: "Before cleanup" });
    });

    expect(result.current.loading).toBe(false);
    expect(result.current.result).toBeNull();
    expect(result.current.error).toBe(message);
  });
});
