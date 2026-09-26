// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { EmptyFoldersPage } from "./EmptyFoldersPage";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import type { StorageStatus } from "../../shared/storage/types";
const invoke=vi.hoisted(()=>vi.fn());
vi.mock("@tauri-apps/api/core",()=>({invoke}));
const id=(n:number)=>n.toString(16).padStart(32,"0");
beforeEach(()=>{
  Object.defineProperties(HTMLDialogElement.prototype,{
    showModal:{configurable:true,value(this:HTMLDialogElement){this.setAttribute("open","");}},
    close:{configurable:true,value(this:HTMLDialogElement){this.removeAttribute("open");}},
  });
  invoke.mockImplementation(async(command,bytes)=>{
    const input=JSON.parse(new TextDecoder().decode(bytes));
    if(command === "choose_storage_root")return {module:"emptyFolders",rootId:id(10),displayPath:"fixture-root"};
    if(command === "start_empty_folders"){expect(input).toEqual({rootId:id(10),depth:20});return id(1);}
    if(command === "storage_scan_status")return {snapshotId:id(1),module:"emptyFolders",phase:"complete",visitedEntries:3,retainedRecords:1,hashedBytes:0,completedHashes:0,completeness:{reasons:[]}} satisfies StorageStatus;
    if(command === "storage_scan_page")return {snapshotId:id(1),records:[{kind:"emptyFolder",record:{recordId:id(2),displayPath:"empty-child",depth:1,descendantDirectories:0,completeness:{reasons:[]},eligibility:{kind:"eligible",candidate_id:id(3)}}}],nextCursor:null,retainedTotal:1,completeness:{reasons:[]}};
    if(command === "create_storage_plan"){expect(input).toEqual({selection:{module:"emptyFolders",snapshotId:id(1),candidateIds:[id(3)]},disposition:"permanent"});return {planId:id(5),disposition:"permanent",selectedCount:1,selectedBytes:0};}
    if(command === "release_storage_scan")return;
    throw Error(`Forbidden native action: ${command}`);
  });
});
afterEach(()=>{cleanup();expect(invoke.mock.calls.some(([name])=>/execute|undo/.test(name))).toBe(false);vi.clearAllMocks();Reflect.deleteProperty(HTMLDialogElement.prototype,"showModal");Reflect.deleteProperty(HTMLDialogElement.prototype,"close");});
async function scan(){fireEvent.click(screen.getByRole("button",{name:"Choose folder"}));await waitFor(()=>expect(screen.getByRole<HTMLButtonElement>("button",{name:"Scan for empty folders"}).disabled).toBe(false));fireEvent.click(screen.getByRole("button",{name:"Scan for empty folders"}));await screen.findByRole("checkbox",{name:"empty-child"});}
it("explains empty-only/root-retention/no-undo rules and reviews only explicitly selected IDs",async()=>{
  render(<EmptyFoldersPage/>);await scan();
  expect(screen.getByText(/chosen root is always retained/)).toBeTruthy();
  expect(screen.getByRole("note").textContent).toContain("A new child blocks removal");
  expect(screen.getByRole<HTMLInputElement>("checkbox",{name:"empty-child"}).checked).toBe(false);
  expect(screen.queryByRole("button",{name:/Review: Move/})).toBeNull();
  fireEvent.click(screen.getByRole("checkbox",{name:"empty-child"}));
  fireEvent.click(screen.getByRole("button",{name:"Review: Delete permanently"}));
  await screen.findByRole("dialog");
  expect(screen.getByRole("button",{name:"Continue to Windows confirmation"})).toBeTruthy();
  fireEvent.click(screen.getByRole("button",{name:"Cancel review"}));
  expect(screen.queryByRole("dialog")).toBeNull();
});
// Pinned empty-folder-cleaner.ipc.ts safeOptions at Kudu db09e051d0615121e659db187e3799438acbc9e6.
it("uses pinned Kudu empty-folder traversal default",()=>{
  render(<EmptyFoldersPage/>);
  expect(screen.getByLabelText<HTMLInputElement>("Scan depth").value).toBe("20");
});
it("invalid depth clears results and cannot start another scan",async()=>{
  render(<EmptyFoldersPage/>);await scan();fireEvent.click(screen.getByRole("checkbox",{name:"empty-child"}));
  fireEvent.change(screen.getByLabelText("Scan depth"),{target:{value:"65"}});
  expect(screen.getByRole("alert").textContent).toContain("0 to 64");
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.getByRole<HTMLButtonElement>("button",{name:"Scan for empty folders"}).disabled).toBe(true);
});
it("renders Spanish copy inside an es-MX provider",()=>{
  render(<I18nProvider languages={["es-MX"]}><EmptyFoldersPage/></I18nProvider>);
  expect(screen.getByRole("heading",{name:"Carpetas vacías"})).toBeTruthy();
  expect(screen.getByRole("button",{name:"Elegir carpeta"})).toBeTruthy();
  expect(screen.getByLabelText("Profundidad de análisis")).toBeTruthy();
});
