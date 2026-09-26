// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DuplicatesPage } from "./DuplicatesPage";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { parseFields } from "./api";
import type { StorageStatus } from "../../shared/storage/types";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const id = (n: number) => n.toString(16).padStart(32, "0");
const member = (n: number) => ({ kind: "duplicateMember", record: { groupId: id(2), file: { recordId: id(n), displayPath: `copy-${n}`, logicalBytes: 9, allocatedBytes: null, modifiedUnixSeconds: null, eligibility: { kind: "eligible", candidate_id: id(n + 100) } } } });
beforeEach(() => {
  invoke.mockImplementation(async (command, bytes) => {
    const input = JSON.parse(new TextDecoder().decode(bytes));
    if (command === "choose_storage_root") { expect(input.module).toBe("duplicates"); return { module: "duplicates", rootId: id(10), displayPath: "fixture" }; }
    if (command === "start_duplicates") { expect(input).toEqual({ rootId: id(10), depth: 20, minimumBytes: 1_048_576, maximumBytes: null, extensions: [] }); return id(1); }
    if (command === "storage_scan_status") return { snapshotId: id(1), module: "duplicates", phase: "complete", visitedEntries: 3, retainedRecords: 4, hashedBytes: 54, completedHashes: 3, completeness: { reasons: [] } } satisfies StorageStatus;
    if (command === "storage_scan_page") return { snapshotId: id(1), completeness: { reasons: [] }, retainedTotal: 3, nextCursor: input.collection === "duplicateMembers" && !input.cursor ? id(99) : null, records: input.collection === "duplicateGroups" ? [{ kind: "duplicateGroup", record: { groupId: id(2), memberCount: 3, independentCopies: 3, bytesPerCopy: 9, completeness: { reasons: [] } } }] : input.cursor ? [member(3), member(5)] : [member(3), member(4)] };
    if (command === "release_storage_scan") return;
    throw Error(`Forbidden: ${command}`);
  });
});
afterEach(() => { cleanup(); expect(invoke.mock.calls.some(([name]) => /execute|undo/.test(name))).toBe(false); vi.clearAllMocks(); });
it("reserves first-page keeper across pages and clears group and filter selections", async () => {
  render(<DuplicatesPage />);
  fireEvent.click(screen.getByText("Choose folder"));
  await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan for duplicates" }).disabled).toBe(false));
  fireEvent.click(screen.getByText("Scan for duplicates"));
  fireEvent.click(await screen.findByText("Review group"));
  await waitFor(() => expect(screen.getByRole<HTMLInputElement>("checkbox", { name: "copy-4" }).disabled).toBe(false));
  expect(screen.getByRole<HTMLInputElement>("checkbox", { name: "copy-3" }).disabled).toBe(true);
  fireEvent.click(screen.getByRole("checkbox", { name: "copy-4" }));
  fireEvent.click(screen.getByText("Next page"));
  await screen.findByRole("checkbox", { name: "copy-5" });
  expect(screen.getByRole<HTMLInputElement>("checkbox", { name: "copy-3" }).disabled).toBe(true);
  expect(screen.queryByRole("checkbox", { name: "copy-4" })).toBeNull();
  fireEvent.click(screen.getByText("Back to groups (clear selection)"));
  await screen.findByText("Review group");
  expect(screen.getByText(/0 selected/)).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Minimum size (MiB)"), { target: { value: "2" } });
  expect(screen.queryByText("Review group")).toBeNull();
});
// Pinned duplicate-finder.ipc.ts safeOptions at Kudu db09e051d0615121e659db187e3799438acbc9e6.
it("uses pinned Kudu duplicate threshold and traversal defaults", () => {
  render(<DuplicatesPage />);
  expect(screen.getByLabelText<HTMLInputElement>("Minimum size (MiB)").value).toBe("1");
  expect(screen.getByLabelText<HTMLInputElement>("Scan depth").value).toBe("20");
});
it("bounds depth and minimum byte conversion", () => {
  expect(parseFields("64", "0")).toEqual({ depth: 64, minimumBytes: 0, maximumBytes: null, extensions: [] });
  expect(parseFields("2", "1", "3", ".TXT, txt, bin")).toEqual({ depth: 2, minimumBytes: 1_048_576, maximumBytes: 3_145_728, extensions: ["txt", "bin"] });
  for (const maximum of ["-1", "0", "Infinity", "0.0000001"]) expect(parseFields("2", "1", maximum)).toBeNull();
  for (const extensions of ["../txt", "a".repeat(33), Array.from({ length: 65 }, (_, i) => `ext${i}`).join(",")]) expect(parseFields("2", "0", "", extensions)).toBeNull();
  for (const [d, m] of [["65", "0"], ["-1", "0"], ["1", "-1"], ["1", "0.0000001"], ["", "0"]]) expect(parseFields(d, m)).toBeNull();
});
it("renders Spanish copy inside an es-MX provider",()=>{
  render(<I18nProvider languages={["es-MX"]}><DuplicatesPage/></I18nProvider>);
  expect(screen.getByRole("heading",{name:"Archivos duplicados"})).toBeTruthy();
  expect(screen.getByLabelText("Tamaño mínimo (MiB)")).toBeTruthy();
  expect(screen.getByRole("button",{name:"Buscar duplicados"})).toBeTruthy();
});
