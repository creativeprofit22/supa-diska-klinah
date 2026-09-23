// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DiskAnalyzerPage } from "./DiskAnalyzerPage";
import { startDiskAnalyzer } from "./api";

const invoke = vi.hoisted(()=>vi.fn());
vi.mock("@tauri-apps/api/core",()=>({invoke}));
const id=(n:number)=>n.toString(16).padStart(32,"0");
const choice={rootId:id(10),module:"diskAnalyzer",displayPath:"C:\\Disposable fixture"};
const directory=(nodeId=id(100), parentId:string|null=null, displayPath=choice.displayPath)=>({kind:"directory",record:{nodeId,parentId,displayPath,logicalBytes:222,allocatedBytes:null,independentFiles:207,hardLinkEntries:0,completeness:{reasons:[]}}});
const status=(snapshotId=id(1),phase="complete")=>({snapshotId,module:"diskAnalyzer",phase,visitedEntries:413,retainedRecords:209,hashedBytes:0,completedHashes:0,completeness:{reasons:[]}});
const page=(records:ReturnType<typeof directory>[], snapshotId=id(1), nextCursor:string|null=null)=>({snapshotId,records,nextCursor,retainedTotal:records.length,completeness:{reasons:[]}});
let chooser: ()=>Promise<typeof choice|null>;
let getStatus: (snapshot:string)=>Promise<ReturnType<typeof status>>;
let getPage: (request:{snapshotId:string;parentId?:string;cursor?:string;collection:string})=>Promise<object>;
let startCount=0;
let failStart=false;
beforeEach(()=>{
  startCount=0; failStart=false; chooser=async()=>choice; getStatus=async snapshot=>status(snapshot);
  getPage=async request=>request.collection === "extensions" ? {snapshotId:request.snapshotId,records:[{kind:"extension",record:{extension:"dat",fileCount:205,logicalBytes:205,allocatedBytes:null,completeness:{reasons:[]}}}],nextCursor:null,retainedTotal:1,completeness:{reasons:[]}} : !request.parentId ? page([directory()],request.snapshotId) : request.parentId === id(100) ? page([directory(id(101),id(100),"C:\\Disposable fixture\\Child")],request.snapshotId,id(99)) : page([],request.snapshotId);
  invoke.mockImplementation(async(command:string,bytes:Uint8Array)=>{
    // TextEncoder's bytes can originate in Node's realm rather than jsdom's.
    expect(Object.prototype.toString.call(bytes)).toBe("[object Uint8Array]");
    const input=JSON.parse(new TextDecoder().decode(bytes));
    if(command === "choose_storage_root") {expect(input).toEqual({module:"diskAnalyzer"});return chooser();}
    if(command === "start_disk_analyzer") { if(failStart) throw {code:"busy"}; return id(++startCount); }
    if(command === "storage_scan_status") return getStatus(input.snapshotId);
    if(command === "storage_scan_page") return getPage(input);
    if(command === "release_storage_scan" || command === "cancel_storage_scan") return;
    throw new Error(`Unexpected IPC: ${command}`);
  });
});
afterEach(()=>{cleanup();expect(invoke.mock.calls.some(([command])=>/cleanup|plan|undo|large_file/.test(command))).toBe(false);vi.clearAllMocks();});
async function start(){fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));await waitFor(()=>expect(screen.getByRole<HTMLButtonElement>("button",{name:"Analyze folder"}).disabled).toBe(false));fireEvent.click(screen.getByRole("button",{name:"Analyze folder"}));await screen.findByRole("region",{name:"Scanned root totals"});}

it("renders native-shaped totals, global extensions and read-only breadcrumb navigation",async()=>{
  render(<DiskAnalyzerPage/>); await start();
  expect(screen.getByRole("region",{name:"Scanned root totals"}).textContent).toContain("222 B");
  expect(screen.getByRole("region",{name:"Scanned root totals"}).textContent).toContain("Unknown allocation");
  fireEvent.click(await screen.findByRole("button",{name:"Child"}));
  await screen.findByText(/No child folders within the displayed depth/);
  expect(screen.getByRole("heading",{name:"Child folders of Child"})).toBe(document.activeElement);
  fireEvent.click(screen.getByRole("button",{name:"Scanned root"}));await screen.findByRole("button",{name:"Child"});
  fireEvent.click(screen.getByRole("button",{name:"Extensions"}));
  await screen.findByText(".dat");expect(screen.getByRole("heading",{name:"Extensions across the scanned root"})).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();expect(screen.queryByRole("button",{name:/delete|cleanup|review/i})).toBeNull();
});
it("sends only validated display depth and an opaque root ID",async()=>{
  render(<DiskAnalyzerPage/>);fireEvent.change(screen.getByLabelText("Displayed folder depth"),{target:{value:"1"}});await start();
  const call=invoke.mock.calls.find(([command])=>command === "start_disk_analyzer")!;
  expect(JSON.parse(new TextDecoder().decode(call[1]))).toEqual({rootId:id(10),displayedDepth:1});
  const count=invoke.mock.calls.length;
  await expect(startDiskAnalyzer(id(10),65)).rejects.toEqual({code:"invalid_input"});
  await expect(startDiskAnalyzer("C:\\Windows",1)).rejects.toEqual({code:"invalid_input"});
  expect(invoke.mock.calls).toHaveLength(count);
  fireEvent.change(screen.getByLabelText("Displayed folder depth"),{target:{value:"65"}});
  expect(screen.getByRole("alert").textContent).toContain("0 to 64");expect(screen.queryByRole("region",{name:"Scanned root totals"})).toBeNull();
});
it("retires the unused root authorization when native start is busy",async()=>{
  failStart=true;render(<DiskAnalyzerPage/>);
  fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));
  await waitFor(()=>expect(screen.getByRole<HTMLButtonElement>("button",{name:"Analyze folder"}).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button",{name:"Analyze folder"}));
  await screen.findByText(/Another scan is active/);
  expect(invoke.mock.calls.some(([c,b])=>c === "release_storage_scan" && JSON.parse(new TextDecoder().decode(b)).snapshotId === choice.rootId)).toBe(true);
});
it("native picker cancellation keeps the existing scope and scan results",async()=>{
  render(<DiskAnalyzerPage/>);await start();await screen.findByRole("button",{name:"Child"});
  chooser=async()=>null;fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));
  await waitFor(()=>expect(screen.getByRole<HTMLButtonElement>("button",{name:"Choose folder"}).disabled).toBe(false));
  expect(screen.getByRole("region",{name:"Scanned root totals"}).textContent).toContain("222 B");
  expect(screen.getByRole("button",{name:"Child"})).toBeTruthy();
});
it("cancels an active scan and ignores the late native status",async()=>{
  let resolve!: (value:ReturnType<typeof status>)=>void;
  getStatus=()=>new Promise(r=>{resolve=r;});render(<DiskAnalyzerPage/>);
  fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));await waitFor(()=>expect(screen.getByRole<HTMLButtonElement>("button",{name:"Analyze folder"}).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button",{name:"Analyze folder"}));
  await waitFor(()=>expect(invoke.mock.calls.some(([c])=>c === "storage_scan_status")).toBe(true));
  fireEvent.click(screen.getByRole("button",{name:"Cancel scan"}));
  await screen.findByText(/Scan cancelled/);await act(async()=>resolve(status()));
  expect(screen.queryByRole("region",{name:"Scanned root totals"})).toBeNull();
  expect(invoke.mock.calls.some(([c])=>c === "cancel_storage_scan")).toBe(true);
});
it("replaces a page and ignores an older page response after a new scope",async()=>{
  render(<DiskAnalyzerPage/>);await start();await screen.findByRole("button",{name:"Child"});
  const original=getPage;let resolve!: (value:object)=>void;
  getPage=request=>request.cursor ? new Promise(r=>{resolve=r;}) : original(request);
  fireEvent.click(screen.getByRole("button",{name:"Next page"}));
  await screen.findByText("Loading page…");
  chooser=async()=>({...choice,rootId:id(20),displayPath:"C:\\Second fixture"});
  fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));
  await screen.findByText("Selected folder: C:\\Second fixture");
  await act(async()=>resolve(page([directory(id(999),id(100),"C:\\STALE ROW")])));
  expect(screen.queryByText(/STALE ROW/)).toBeNull();expect(screen.queryByRole("region",{name:"Scanned root totals"})).toBeNull();
  expect(invoke.mock.calls.some(([c,b])=>c === "release_storage_scan" && JSON.parse(new TextDecoder().decode(b)).snapshotId === id(1))).toBe(true);
});
it("hides retained totals when native paging reports an expired snapshot",async()=>{
  render(<DiskAnalyzerPage/>);await start();await screen.findByRole("button",{name:"Child"});
  getPage=async()=>{throw {code:"snapshot_unavailable"};};
  fireEvent.click(screen.getByRole("button",{name:"Next page"}));
  await screen.findByText(/These results expired/);
  expect(screen.queryByRole("region",{name:"Scanned root totals"})).toBeNull();
});
it("releases late picker authorizations on unmount",async()=>{
  let resolve!: (root:typeof choice)=>void;chooser=()=>new Promise(r=>{resolve=r;});
  const {unmount}=render(<DiskAnalyzerPage/>);fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));unmount();
  await act(async()=>resolve(choice));
  expect(invoke.mock.calls.some(([c,b])=>c === "release_storage_scan" && JSON.parse(new TextDecoder().decode(b)).snapshotId === choice.rootId)).toBe(true);
});
