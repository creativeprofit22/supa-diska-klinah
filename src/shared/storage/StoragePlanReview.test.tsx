// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { StoragePlanReview } from "./StoragePlanReview";
import type { CleanupExecutionSummary } from "../cleanup/api";
import type { StorageSelection } from "./types";

const selection: StorageSelection = {module:"largeFiles", snapshotId:"a".repeat(32), candidateIds:["b".repeat(32)]};
const summary = {planId:"c".repeat(32), disposition:"recycleBin" as const, selectedCount:1, selectedBytes:512};
const execution: CleanupExecutionSummary = {executionId:"e".repeat(32), planId:summary.planId, disposition:"recycleBin", completed:true, items:[{itemId:"f".repeat(32),state:"recycled",logicalBytes:512}], accounting:{selectedBytes:512,processedBytes:512,failedBytes:0,quarantinedBytes:0,purgedBytes:0,occupiedBytes:512,reclaimedBytes:0}};
const invoke = vi.hoisted(() => vi.fn().mockRejectedValue(new Error("Native IPC forbidden in fixture tests")));
vi.mock("@tauri-apps/api/core", () => ({invoke}));
function setup() {
  const createPlan = vi.fn().mockResolvedValue(summary);
  const actions = {executeCleanupPlan:vi.fn().mockResolvedValue(execution),executePermanentCleanupPlan:vi.fn().mockResolvedValue({...execution,disposition:"permanent"}),cleanupHistory:vi.fn().mockResolvedValue({records:[execution],nextCursor:null}),undoCleanup:vi.fn().mockResolvedValue({...execution,items:[{...execution.items[0],state:"restored"}]})};
  const onExecuted = vi.fn();
  return {createPlan, actions, onExecuted};
}
beforeEach(() => {
  // jsdom has no top layer. Real modality, focus trap and Escape are browser-tested.
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.setAttribute("open", ""); } },
    close: { configurable: true, value(this: HTMLDialogElement) { this.removeAttribute("open"); } },
  });
});
afterEach(() => {cleanup(); expect(invoke).not.toHaveBeenCalled(); Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal"); Reflect.deleteProperty(HTMLDialogElement.prototype, "close"); vi.restoreAllMocks();});
it("renders mixed zero-byte failures and uncertain outcomes independently of accounting", async () => {
  const props = setup();
  props.actions.executeCleanupPlan.mockResolvedValue({...execution, items: [
    execution.items[0],
    {itemId:"zero-folder", displayPath:"fixture/empty-child", state:"failed", logicalBytes:0, failure:"permanent-remove-failed"},
    {itemId:"uncertain", state:"unknown", logicalBytes:0, failure:"recovery-identity-unproven"},
    {itemId:"unconfirmed", state:"unknown", logicalBytes:0, failure:null},
    {itemId:"locked", state:"failed", logicalBytes:0, failure:"in-use"},
    {itemId:"vanished", state:"failed", logicalBytes:0, failure:"not-found"},
    {itemId:"denied", state:"failed", logicalBytes:0, failure:"permission-denied"},
  ]});
  render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to Recycle Bin"}));
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button",{name:"Move to Recycle Bin"}));
  const outcome = within(await screen.findByRole("region",{name:"Latest storage cleanup outcome"}));
  expect(outcome.getByText(/4 failed items.*2 uncertain items/)).toBeTruthy();
  expect(outcome.getByText(/open in another program/)).toBeTruthy();
  expect(outcome.getByText(/already gone when cleanup reached it/)).toBeTruthy();
  expect(outcome.getByText(/Windows denied access/)).toBeTruthy();
  expect(outcome.getByText("fixture/empty-child")).toBeTruthy();
  expect(outcome.getByText(/The result is not confirmed/)).toBeTruthy();
  expect(outcome.getByText(/Removal was refused/)).toBeTruthy();
  expect(outcome.getByText(/Recovery identity could not be verified/)).toBeTruthy();
  expect(outcome.getByText(/zero-folder/)).toBeTruthy();
  expect(outcome.getByText(/0 B reclaimed · 0 B failed · 512 B still occupied/)).toBeTruthy();
  expect(props.actions.executeCleanupPlan).toHaveBeenCalledTimes(1);
  expect(props.actions.undoCleanup).not.toHaveBeenCalled();
});
it.each([
  ["restore-rejected", /Restoration was refused/],
  ["restore-protection-rejected", /Restoration was blocked by current protection/],
] as const)("shows %s undo without offering another recovery attempt", async (failure, reason) => {
  const props = setup();
  props.actions.undoCleanup.mockResolvedValue({...execution, items:[{...execution.items[0], state:"quarantined", failure}]});
  render(<StoragePlanReview selection={null} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Load cleanup history"}));
  fireEvent.click(await screen.findByRole("button",{name:"Undo cleanup"}));
  const outcome = within(await screen.findByRole("region",{name:"Latest storage cleanup outcome"}));
  expect(outcome.getByText(/1 failed item/)).toBeTruthy();
  expect(outcome.getByText(reason)).toBeTruthy();
  expect(screen.queryByRole("button",{name:"Undo cleanup"})).toBeNull();
  expect(props.actions.undoCleanup).toHaveBeenCalledTimes(1);
});
it("replaces bounded item detail pages without changing execution history", async () => {
  const props = setup();
  props.actions.cleanupHistory.mockResolvedValue({records:[{...execution,items:Array.from({length:45},(_,i)=>({...execution.items[0],itemId:`item-${i}`, failure:"untrusted OS error"}))}],nextCursor:null});
  render(<StoragePlanReview selection={null} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Load cleanup history"}));
  fireEvent.click(await screen.findByText("Item outcomes"));
  const details = within(screen.getByRole("group",{name:"Item outcomes"}));
  expect(await details.findAllByRole("listitem")).toHaveLength(20);
  expect(details.queryByText(/untrusted OS error/)).toBeNull();
  fireEvent.click(details.getByRole("button",{name:"Next items"}));
  expect(details.getAllByRole("listitem")).toHaveLength(20);
  expect(details.queryByText(/^Item ID: item-0$/)).toBeNull();
  fireEvent.click(details.getByRole("button",{name:"Next items"}));
  expect(details.getAllByRole("listitem")).toHaveLength(5);
  expect(props.actions.cleanupHistory).toHaveBeenCalledTimes(1);
  expect(props.actions.undoCleanup).not.toHaveBeenCalled();
});
it("reviews only explicit selections and does not execute on preparation or dismissal", async () => {
  const props = setup(); render(<StoragePlanReview selection={selection} {...props}/>);
  const review = screen.getByRole("button",{name:"Review: Move to Recycle Bin"}); review.focus(); fireEvent.click(review);
  await screen.findByRole("dialog");
  expect(props.createPlan).toHaveBeenCalledWith(selection,"recycleBin");
  expect(document.activeElement).toBe(screen.getByRole("button",{name:"Cancel review"}));
  fireEvent.click(screen.getByRole("button",{name:"Cancel review"}));
  expect(screen.queryByRole("dialog")).toBeNull(); expect(document.activeElement).toBe(review);
  expect(props.actions.executeCleanupPlan).not.toHaveBeenCalled();
});
it("discards a late plan after selection changes", async () => {
  const props = setup(); let resolve!: (value: typeof summary) => void;
  props.createPlan.mockImplementationOnce(() => new Promise(r => {resolve=r;}));
  const {rerender} = render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to Recycle Bin"}));
  rerender(<StoragePlanReview selection={{...selection,snapshotId:"d".repeat(32)}} {...props}/>);
  await act(async()=>{resolve(summary);});
  expect(screen.queryByRole("dialog")).toBeNull(); expect(props.actions.executeCleanupPlan).not.toHaveBeenCalled();
});
it("revokes a visible plan when scan cancellation clears selection", async () => {
  const props = setup(); const {rerender} = render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to Recycle Bin"})); await screen.findByRole("dialog");
  rerender(<StoragePlanReview selection={null} {...props}/>);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole<HTMLButtonElement>("button",{name:"Review: Move to Recycle Bin"}).disabled).toBe(true);
});
it("uses the immutable plan ID once, with truthful reclaimed bytes and existing history/undo APIs", async () => {
  const props = setup(); render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to Recycle Bin"})); await screen.findByRole("dialog");
  const confirm = screen.getByRole("button",{name:"Move to Recycle Bin"});
  fireEvent.click(confirm); fireEvent.click(confirm);
  await screen.findByRole("heading",{name:"Cleanup finished"});
  expect(props.actions.executeCleanupPlan).toHaveBeenCalledExactlyOnceWith(summary.planId);
  expect(screen.getByText(/0 B reclaimed · 0 B failed · 512 B still occupied/)).toBeTruthy();
  await waitFor(()=>expect(props.actions.cleanupHistory).toHaveBeenCalledTimes(1));
  await waitFor(()=>expect(screen.getByRole<HTMLButtonElement>("button",{name:"Undo cleanup"}).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button",{name:"Undo cleanup"}));
  await waitFor(()=>expect(props.actions.undoCleanup).toHaveBeenCalledExactlyOnceWith(execution.executionId));
  await waitFor(()=>expect(screen.queryByRole("button",{name:"Undo cleanup"})).toBeNull());
});
it("retained history regression exposes older cleanup rows without executing", async () => {
  const props = setup();
  const cursor = JSON.stringify({version:1,kind:"cleanup",timestamp:1,id:"a".repeat(32)});
  const oldest = {...execution,executionId:"1".repeat(32)};
  props.actions.cleanupHistory.mockResolvedValueOnce({records:Array.from({length:20}, (_, index) => ({...execution, executionId:index.toString(16).padStart(32,"0")})),nextCursor:cursor}).mockResolvedValue({records:[oldest],nextCursor:null});
  props.actions.undoCleanup.mockResolvedValue({...oldest,items:[{...oldest.items[0],state:"restored"}]});
  render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Load cleanup history"}));
  await waitFor(()=>expect(props.actions.cleanupHistory).toHaveBeenCalledTimes(1));
  await waitFor(()=>expect(screen.getAllByRole("button",{name:"Undo cleanup"})).toHaveLength(20));
  fireEvent.click(screen.getByRole("button",{name:/older/i}));
  await waitFor(()=>expect(screen.getAllByRole("button",{name:"Undo cleanup"})).toHaveLength(1));
  expect(props.actions.cleanupHistory).toHaveBeenLastCalledWith({cursor,limit:20});
  fireEvent.click(screen.getByRole("button",{name:"Undo cleanup"}));
  await waitFor(()=>expect(screen.queryByRole("button",{name:"Undo cleanup"})).toBeNull());
  expect(props.actions.undoCleanup).toHaveBeenCalledExactlyOnceWith(oldest.executionId);
  expect(props.actions.cleanupHistory).toHaveBeenCalledTimes(2);
  expect(screen.getByText("End of cleanup history.")).toBeTruthy();
  expect(props.actions.executeCleanupPlan).not.toHaveBeenCalled();
});

it("permanent disposition uses only the separate native-confirmed command", async () => {
  const props = setup(); props.createPlan.mockResolvedValue({...summary,disposition:"permanent"});
  render(<StoragePlanReview selection={selection} dispositions={["permanent"]} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Delete permanently"})); await screen.findByRole("dialog");
  expect(screen.getByText(/separate Windows confirmation/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button",{name:"Continue to Windows confirmation"}));
  await waitFor(()=>expect(props.actions.executePermanentCleanupPlan).toHaveBeenCalledExactlyOnceWith(summary.planId));
  expect(props.actions.executeCleanupPlan).not.toHaveBeenCalled();
});
it.each(["largeFiles", "duplicates", "cleaner", "browser"] as const)("%s refuses unsupported recovery without silently choosing permanent deletion", async module => {
  const props = setup();
  props.createPlan.mockRejectedValueOnce({code:"recovery_volume_unsupported",path:"private native path"})
    .mockResolvedValue({...summary,disposition:"permanent"});
  const selected = {...selection,module};
  const {rerender} = render(<StoragePlanReview selection={selected} dispositions={["quarantine","permanent"]} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to app recovery"}));
  expect((await screen.findByRole("alert")).textContent).toContain("App recovery is unavailable for the selected volume");
  expect(screen.getByRole("alert").textContent).not.toContain("private");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole<HTMLButtonElement>("button",{name:"Review: Move to app recovery"}).disabled).toBe(true);
  expect(props.createPlan).toHaveBeenCalledExactlyOnceWith(selected,"quarantine");
  expect(props.actions.executeCleanupPlan).not.toHaveBeenCalled();
  expect(props.actions.executePermanentCleanupPlan).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"Review: Delete permanently"}));
  await screen.findByRole("dialog");
  expect(props.actions.executePermanentCleanupPlan).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"Continue to Windows confirmation"}));
  await waitFor(()=>expect(props.actions.executePermanentCleanupPlan).toHaveBeenCalledExactlyOnceWith(summary.planId));
  expect(props.actions.executeCleanupPlan).not.toHaveBeenCalled();
  rerender(<StoragePlanReview selection={{...selected,snapshotId:"d".repeat(32)}} dispositions={["quarantine","permanent"]} {...props}/>);
  expect(screen.getByRole<HTMLButtonElement>("button",{name:"Review: Move to app recovery"}).disabled).toBe(false);
});
it("empty folders remain permanent-only", () => {
  render(<StoragePlanReview selection={{...selection,module:"emptyFolders"}} dispositions={["permanent"]} {...setup()}/>);
  expect(screen.queryByRole("button",{name:"Review: Move to app recovery"})).toBeNull();
  expect(screen.getByRole("button",{name:"Review: Delete permanently"})).toBeTruthy();
});
it("native confirmation cancellation/failure never reports success or replays a plan", async () => {
  const props = setup(); props.actions.executeCleanupPlan.mockRejectedValue({code:"native_failure",path:"private"});
  render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to Recycle Bin"})); await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button",{name:"Move to Recycle Bin"}));
  await screen.findByRole("alert");
  expect(screen.getByRole("alert").textContent).toContain("Load history");
  expect(screen.getByRole("alert").textContent).not.toContain("private");
  expect(screen.queryByRole("dialog")).toBeNull(); expect(props.onExecuted).not.toHaveBeenCalled();
});
it("reports completed-but-failed native jobs as needing attention, not successful cleanup", async () => {
  const props = setup();
  props.actions.executeCleanupPlan.mockResolvedValue({...execution,items:[{...execution.items[0],state:"failed",failure:"identity-required-recycle-unsupported"}],accounting:{...execution.accounting,processedBytes:0,failedBytes:512}});
  props.actions.cleanupHistory.mockResolvedValue({records:[],nextCursor:null});
  render(<StoragePlanReview selection={selection} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"Review: Move to Recycle Bin"})); await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button",{name:"Move to Recycle Bin"}));
  await screen.findByRole("heading",{name:"Cleanup needs attention"});
  expect(screen.queryByRole("heading",{name:"Cleanup finished"})).toBeNull();
  expect(screen.queryByRole("button",{name:"Undo cleanup"})).toBeNull();
});
