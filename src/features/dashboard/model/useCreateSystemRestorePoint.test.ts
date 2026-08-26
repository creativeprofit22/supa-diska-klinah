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

  it("keeps native failures non-sensitive and recoverable", async () => {
    createSystemRestorePoint.mockRejectedValue({ code: "operationUnavailable" });
    const { result } = renderHook(() => useCreateSystemRestorePoint());

    await act(async () => {
      await result.current.create({ description: "Before cleanup" });
    });

    expect(result.current.loading).toBe(false);
    expect(result.current.result).toBeNull();
    expect(result.current.error).toBe(
      "Windows did not complete the restore point. This app is still open.",
    );
  });
});
