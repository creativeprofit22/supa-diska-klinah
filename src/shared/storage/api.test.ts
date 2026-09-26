import { afterEach, expect, it, vi } from "vitest";
import { authorizeStorageScope, cancelStorageScan, chooseStorageRoot, createStoragePlan, listStorageScopes, releaseStorageScan, storageError, storagePage, storageStatus } from "./api";
const invoke = vi.hoisted(() => vi.fn().mockResolvedValue(null));
vi.mock("@tauri-apps/api/core", () => ({invoke}));
afterEach(()=>vi.clearAllMocks());
it("sends Raw UTF-8 JSON to all eight real storage commands",async()=>{
  const input={module:"largeFiles" as const,snapshotId:"a".repeat(32)};
  await chooseStorageRoot("largeFiles"); await listStorageScopes("cleaner"); await authorizeStorageScope("cleaner","b".repeat(32));
  await storageStatus(input); await storagePage({...input,collection:"files",pageSize:100});
  await cancelStorageScan(input); await releaseStorageScan(input);
  await createStoragePlan({...input,candidateIds:["c".repeat(32)]},"recycleBin");
  expect(invoke.mock.calls.map(call=>call[0])).toEqual(["choose_storage_root","list_storage_scopes","authorize_storage_scope","storage_scan_status","storage_scan_page","cancel_storage_scan","release_storage_scan","create_storage_plan"]);
  for(const [,body] of invoke.mock.calls){expect(body).toBeInstanceOf(Uint8Array); expect(()=>JSON.parse(new TextDecoder().decode(body))).not.toThrow();}
});
it("strips extra selection authority and rejects malformed/oversized IDs before IPC",async()=>{
  const input={module:"largeFiles" as const,snapshotId:"a".repeat(32),candidateIds:["b".repeat(32)],path:"C:\\private",proof:{}};
  await createStoragePlan(input,"recycleBin");
  const payload=JSON.parse(new TextDecoder().decode(invoke.mock.calls[0][1]));
  expect(Object.keys(payload.selection).sort()).toEqual(["candidateIds","module","snapshotId"]);
  invoke.mockClear();
  await expect(createStoragePlan({...input,candidateIds:[]},"recycleBin")).rejects.toEqual({code:"invalid_input"});
  await expect(createStoragePlan({...input,candidateIds:Array(1001).fill("b".repeat(32))},"recycleBin")).rejects.toEqual({code:"invalid_input"});
  await expect(createStoragePlan({...input,candidateIds:["b".repeat(32),"b".repeat(32)]},"recycleBin")).rejects.toEqual({code:"invalid_input"});
  expect(invoke).not.toHaveBeenCalled();
});
it("never exposes arbitrary native error text",()=>{
  expect(storageError(new Error("C:\\private"))).not.toContain("private");
  expect(storageError({code:"snapshot_unavailable"})).toContain("expired");
});
it("joins duplicate scope refreshes and bounds cross-module refreshes to the latest queued intent",async()=>{
  let release!:()=>void;
  const waiting=new Promise<void>(resolve=>{release=resolve;});
  invoke.mockImplementationOnce(()=>waiting.then(()=>[])).mockResolvedValue([]);
  const first=listStorageScopes("cleaner");
  expect(listStorageScopes("cleaner")).toBe(first);
  const discarded=listStorageScopes("browser").catch(error=>error);
  const latest=listStorageScopes("cleaner");
  expect(invoke).toHaveBeenCalledTimes(1);
  release();
  await Promise.all([first,latest]);
  expect(await discarded).toEqual({code:"snapshot_unavailable"});
  expect(invoke).toHaveBeenCalledTimes(2);
  expect(invoke.mock.calls.map(([,bytes])=>JSON.parse(new TextDecoder().decode(bytes)).module)).toEqual(["cleaner","cleaner"]);
});
