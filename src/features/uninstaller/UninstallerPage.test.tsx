// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { UninstallerPage } from "./UninstallerPage";
import type { VendorJob, VendorHistoryPage } from "./api";
const invoke=vi.hoisted(()=>vi.fn());
vi.mock("@tauri-apps/api/core",()=>({invoke}));
const id=(n:number)=>n.toString(16).padStart(32,"0");
const job=(state:VendorJob["state"]):VendorJob=>({jobId:id(3),programId:id(2),programName:"Fixture vendor",state,createdAt:1,updatedAt:1,exitCode:state==="completed"?0:null,launchError:null,persistenceError:false,completionMeaning:"launcherNotRemovalProof",leftoverSupport:"unknownOwnership"});
beforeEach(()=>{
 invoke.mockImplementation(async(command,bytes)=>{
  const input=JSON.parse(new TextDecoder().decode(bytes));
  if(command==="vendor_job_history")return {records:[],nextCursor:null} satisfies VendorHistoryPage;
  if(command==="start_program_inventory"){expect(input).toEqual({nameContains:"",largestFirst:false});return id(1);}
  if(command==="storage_scan_status")return{snapshotId:id(1),module:"uninstaller",phase:"complete",visitedEntries:1,retainedRecords:1,hashedBytes:0,completedHashes:0,completeness:{reasons:[]}};
  if(command==="storage_scan_page")return{snapshotId:id(1),records:[{kind:"program",record:{programId:id(2),name:"Fixture vendor",publisher:null,version:null,installDate:null,estimatedSizeBytes:1024,lastUsedAt:null,leftoverSupport:"unknownOwnership"}}],nextCursor:null,retainedTotal:1,completeness:{reasons:[]}};
  if(command==="prepare_vendor_job"){expect(input).toEqual({snapshotId:id(1),programId:id(2)});return job("awaitingConfirmation");}
  if(command==="confirm_vendor_job"){expect(input).toEqual({jobId:id(3)});return job("completed");}
  if(command==="cancel_vendor_job")return job("cancelledBeforeLaunch");
  if(command==="release_vendor_job"||command==="release_storage_scan")return;
  throw Error(`Forbidden command ${command}`);
 });
});
afterEach(()=>{cleanup();expect(invoke.mock.calls.some(([name])=>/cleanup|storage_plan|undo/.test(name))).toBe(false);vi.clearAllMocks();});
it("keeps vendor review/confirmation separate and never claims launcher exit proves removal",async()=>{
 render(<UninstallerPage/>);
 expect(screen.getByText("Refresh inventory to list installed programs.")).toBeTruthy();
 expect(invoke.mock.calls.some(([name])=>name==="prepare_vendor_job"||name==="confirm_vendor_job")).toBe(false);
 fireEvent.click(screen.getByRole("button",{name:"Refresh inventory"}));
 fireEvent.click(await screen.findByRole("button",{name:"Review vendor uninstall for Fixture vendor"}));
 const review = await screen.findByRole("region",{name:"Immutable vendor job review"});
 expect(document.activeElement).toBe(review);
 expect(invoke.mock.calls.some(([name])=>name==="confirm_vendor_job")).toBe(false);
 expect(screen.queryByRole("checkbox")).toBeNull();
 const confirm = screen.getByRole("button",{name:"Continue to Windows confirmation"});
 confirm.focus();
 fireEvent.click(confirm);
 await screen.findByText("Launcher exited successfully. This is not proof that the program was removed.");
 expect(document.activeElement).toBe(review);
 expect(invoke.mock.calls.filter(([name])=>name==="confirm_vendor_job")).toHaveLength(1);
 expect(screen.queryByRole("button",{name:/undo|leftovers/i})).toBeNull();
});
it.each(["missing", "expired"])("removes confirmation for %s evidence after manual history refresh",async evidence=>{
 render(<UninstallerPage/>);
 fireEvent.click(screen.getByRole("button",{name:"Refresh inventory"}));
 fireEvent.click(await screen.findByRole("button",{name:"Review vendor uninstall for Fixture vendor"}));
 await screen.findByRole("button",{name:"Continue to Windows confirmation"});
 if(evidence==="expired")invoke.mockResolvedValueOnce({records:[job("cancelledBeforeLaunch")],nextCursor:null} satisfies VendorHistoryPage);
 fireEvent.click(screen.getByRole("button",{name:"Refresh retained history"}));
 await screen.findByText(/Refresh inventory and review again/);
 expect(screen.queryByRole("button",{name:"Continue to Windows confirmation"})).toBeNull();
 expect(screen.queryByText(/Awaiting separate Windows confirmation/)).toBeNull();
 expect((screen.getByRole("button",{name:"Review vendor uninstall for Fixture vendor"}) as HTMLButtonElement).disabled).toBe(true);
 expect(invoke.mock.calls.filter(([name])=>name==="prepare_vendor_job")).toHaveLength(1);
 expect(invoke.mock.calls.some(([name])=>name==="confirm_vendor_job"||name==="cancel_vendor_job")).toBe(false);
});
it("returns focus to the outcome when a prepared job is cancelled",async()=>{
 render(<UninstallerPage/>);
 fireEvent.click(screen.getByRole("button",{name:"Refresh inventory"}));
 fireEvent.click(await screen.findByRole("button",{name:"Review vendor uninstall for Fixture vendor"}));
 const review = await screen.findByRole("region",{name:"Immutable vendor job review"});
 const cancel = screen.getByRole("button",{name:"Cancel prepared job"});
 cancel.focus(); fireEvent.click(cancel);
 await screen.findByText("Cancelled before launch. No vendor installer was started by this job.");
 expect(document.activeElement).toBe(review);
 expect(invoke.mock.calls.some(([name])=>name==="confirm_vendor_job")).toBe(false);
});
it("inspects older outcomes and distinguishes end, empty, and rejected pages without launch authority",async()=>{
 const cursor=JSON.stringify({version:1,kind:"vendor",timestamp:1,id:id(3)});
 invoke.mockResolvedValueOnce({records:[job("completed")],nextCursor:cursor} satisfies VendorHistoryPage);
 render(<UninstallerPage/>);
 await screen.findByText(/Launcher exited successfully/);
 expect(screen.getByText(/64 retained jobs per page/)).toBeTruthy();
 invoke.mockResolvedValueOnce({records:[{...job("outcomeUnknown"),jobId:id(8),programName:"Older vendor"}],nextCursor:null} satisfies VendorHistoryPage);
 fireEvent.click(screen.getByRole("button",{name:"Older retained jobs"}));
 await screen.findByText("Older vendor");expect(screen.queryByText("Fixture vendor")).toBeNull();
 expect(screen.getByText("End of retained history.")).toBeTruthy();
 const last=invoke.mock.calls.at(-1)!;expect(JSON.parse(new TextDecoder().decode(last[1]))).toEqual({cursor,limit:64});
 invoke.mockResolvedValueOnce({records:[],nextCursor:null} satisfies VendorHistoryPage);
 fireEvent.click(screen.getByRole("button",{name:"Refresh retained history"}));await screen.findByText("No retained jobs reported.");
 invoke.mockResolvedValueOnce({records:Array.from({length:65},()=>job("failed")),nextCursor:null});
 fireEvent.click(screen.getByRole("button",{name:"Refresh retained history"}));await screen.findByRole("alert");
 expect(screen.queryByText("No retained jobs reported.")).toBeNull();
 expect(invoke.mock.calls.every(([command])=>command==="vendor_job_history")).toBe(true);
});
it.each(["history_count_capacity","history_size_capacity"])("explains %s without suggesting deletion",async code=>{
 render(<UninstallerPage/>);fireEvent.click(screen.getByRole("button",{name:"Refresh inventory"}));
 const button=await screen.findByRole("button",{name:"Review vendor uninstall for Fixture vendor"});
 invoke.mockRejectedValueOnce({code});fireEvent.click(button);
 await screen.findByText(/New jobs are stopped until a future journal migration/);
 expect(invoke.mock.calls.some(([name])=>name==="confirm_vendor_job")).toBe(false);
});
it("filter changes invalidate a prepared job without confirming it",async()=>{
 render(<UninstallerPage/>);fireEvent.click(screen.getByRole("button",{name:"Refresh inventory"}));
 fireEvent.click(await screen.findByRole("button",{name:"Review vendor uninstall for Fixture vendor"}));
 await screen.findByRole("region",{name:"Immutable vendor job review"});
 const filter = screen.getByLabelText("Program name"); filter.focus();
 fireEvent.change(filter,{target:{value:"other"}});
 await waitFor(()=>expect(invoke.mock.calls.some(([name])=>name==="release_vendor_job")).toBe(true));
 expect(screen.queryByRole("region",{name:"Immutable vendor job review"})).toBeNull();
 expect(document.activeElement).toBe(filter);
 expect(invoke.mock.calls.some(([name])=>name==="confirm_vendor_job")).toBe(false);
});
