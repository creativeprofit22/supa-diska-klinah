// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useVendorJobs } from "./useVendorJobs";
import type { VendorJob, VendorHistoryPage } from "./api";
const api=vi.hoisted(()=>({prepareVendorJob:vi.fn(),confirmVendorJob:vi.fn(),cancelVendorJob:vi.fn(),releaseVendorJob:vi.fn(),vendorJobStatus:vi.fn(),vendorJobHistory:vi.fn()}));
vi.mock("./api",async original=>({...await original<typeof import("./api")>(),...api}));
const id=(n:number)=>n.toString(16).padStart(32,"0");
const job=(state:VendorJob["state"]="awaitingConfirmation"):VendorJob=>({jobId:id(1),programId:id(2),programName:"Fixture vendor",state,createdAt:1,updatedAt:1,exitCode:null,launchError:null,persistenceError:false,completionMeaning:"fixture",leftoverSupport:"ownershipNotEstablished"});
function deferred<T>(){let resolve!:(value:T)=>void;let reject!:(error:unknown)=>void;const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};}
beforeEach(()=>{api.prepareVendorJob.mockResolvedValue(job());api.confirmVendorJob.mockResolvedValue(job("queued"));api.cancelVendorJob.mockResolvedValue(job("cancelledBeforeLaunch"));api.releaseVendorJob.mockResolvedValue(undefined);api.vendorJobStatus.mockResolvedValue(job("completed"));api.vendorJobHistory.mockResolvedValue({records:[],nextCursor:null} satisfies VendorHistoryPage);});
afterEach(()=>{cleanup();vi.clearAllMocks();vi.useRealTimers();});
it("never prepares or confirms automatically; stale prepare is cancelled and released",async()=>{
 const pending=deferred<VendorJob>();api.prepareVendorJob.mockReturnValueOnce(pending.promise);
 const {result,rerender}=renderHook(({scope})=>useVendorJobs(scope),{initialProps:{scope:"one"}});
 expect(api.prepareVendorJob).not.toHaveBeenCalled();expect(api.confirmVendorJob).not.toHaveBeenCalled();
 act(()=>{void result.current.prepare(id(3),id(2));});rerender({scope:"two"});
 await act(async()=>pending.resolve(job()));
 expect(result.current.job).toBeNull();expect(api.cancelVendorJob).toHaveBeenCalledWith(id(1));expect(api.releaseVendorJob).toHaveBeenCalledWith(id(1));
 expect(api.confirmVendorJob).not.toHaveBeenCalled();
});
it("cancels unsubmitted consent on navigation but never stops an already-submitted vendor",async()=>{
 const {result,rerender,unmount}=renderHook(({scope})=>useVendorJobs(scope),{initialProps:{scope:"one"}});
 await act(async()=>{await result.current.prepare(id(3),id(2));});rerender({scope:"two"});
 await waitFor(()=>expect(api.releaseVendorJob).toHaveBeenCalledTimes(1));
 await act(async()=>{await result.current.prepare(id(4),id(2));await result.current.confirm();});
 api.cancelVendorJob.mockClear();api.releaseVendorJob.mockClear();unmount();
 expect(api.cancelVendorJob).not.toHaveBeenCalled();expect(api.releaseVendorJob).not.toHaveBeenCalled();
});
it("polls serially, ignores cancelled-poll errors, and stops on terminal state",async()=>{
 vi.useFakeTimers();const pending=deferred<VendorJob>();api.vendorJobStatus.mockReturnValueOnce(pending.promise);
 const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();});
 await act(async()=>{await vi.advanceTimersByTimeAsync(3000);});expect(api.vendorJobStatus).toHaveBeenCalledTimes(1);
 await act(async()=>{await result.current.cancel();pending.reject(new Error("old failure"));});
 expect(result.current.job?.state).toBe("cancelledBeforeLaunch");expect(result.current.error).toBeNull();
 await act(async()=>{await vi.advanceTimersByTimeAsync(3000);});expect(api.vendorJobStatus).toHaveBeenCalledTimes(1);
});
it("loads the latest bounded page under StrictMode and ignores the stale setup response",async()=>{
 const pending=deferred<VendorHistoryPage>();
 api.vendorJobHistory.mockReturnValueOnce(pending.promise).mockResolvedValue({records:Array.from({length:64},(_,i)=>({...job("outcomeUnknown"),jobId:id(i+1)})),nextCursor:null} satisfies VendorHistoryPage);
 const {result}=renderHook(()=>useVendorJobs("one"),{reactStrictMode:true});
 expect(api.vendorJobHistory).toHaveBeenCalledTimes(1);
 await act(async()=>pending.resolve({records:[],nextCursor:null}));
 await waitFor(()=>expect(result.current.history).toHaveLength(64));
 expect(result.current.history).toHaveLength(64);expect(api.vendorJobHistory).toHaveBeenCalledTimes(2);
});
it("browses older outcomes without operation authority and refreshes newest independently of review",async()=>{
 const cursor=JSON.stringify({version:1,kind:"vendor",timestamp:1,id:id(1)});
 api.vendorJobHistory.mockResolvedValueOnce({records:[job("completed")],nextCursor:cursor} satisfies VendorHistoryPage);
 const {result,rerender}=renderHook(({scope})=>useVendorJobs(scope),{initialProps:{scope:"one"}});
 await waitFor(()=>expect(result.current.historyLoaded).toBe(true));
 api.vendorJobHistory.mockResolvedValueOnce({records:[{...job("outcomeUnknown"),jobId:id(9)}],nextCursor:null} satisfies VendorHistoryPage);
 await act(async()=>{await result.current.olderHistory();});
 expect(result.current.history.map(row=>row.jobId)).toEqual([id(9)]);
 expect(api.vendorJobHistory).toHaveBeenLastCalledWith({cursor,limit:64});
 rerender({scope:"two"});expect(api.vendorJobHistory).toHaveBeenCalledTimes(2);
 await act(async()=>{await result.current.refreshHistory();});
 expect(api.vendorJobHistory).toHaveBeenLastCalledWith({cursor:null,limit:64});
 for(const command of [api.prepareVendorJob,api.confirmVendorJob,api.cancelVendorJob,api.releaseVendorJob,api.vendorJobStatus]) expect(command).not.toHaveBeenCalled();
});
it("ignores stale errors, isolates current errors, and ignores unmounted requests",async()=>{
 const stale=deferred<VendorHistoryPage>();api.vendorJobHistory.mockReturnValueOnce(stale.promise);
 const {result,unmount}=renderHook(()=>useVendorJobs("one"),{reactStrictMode:true});
 await act(async()=>{stale.reject(new Error("stale"));});
 expect(result.current.historyError).toBeNull();expect(result.current.historyLoaded).toBe(true);
 api.vendorJobHistory.mockRejectedValueOnce(new Error("unreadable"));
 await act(async()=>{await result.current.refreshHistory();});
 expect(result.current.historyError).toMatch(/could not be loaded/);expect(result.current.error).toBeNull();
 const late=deferred<VendorHistoryPage>();api.vendorJobHistory.mockReturnValueOnce(late.promise);
 act(()=>{void result.current.refreshHistory();});unmount();
 const calls=api.vendorJobHistory.mock.calls.length;
 await act(async()=>late.resolve({records:[job("failed")],nextCursor:null}));
 expect(api.vendorJobHistory).toHaveBeenCalledTimes(calls);
 expect(api.prepareVendorJob).not.toHaveBeenCalled();expect(api.confirmVendorJob).not.toHaveBeenCalled();
});
it("does not refresh history for preparation but refreshes on a terminal transition",async()=>{
 const {result}=renderHook(()=>useVendorJobs("one"));
 await waitFor(()=>expect(result.current.historyLoaded).toBe(true));
 await act(async()=>{await result.current.prepare(id(3),id(2));});
 expect(api.vendorJobHistory).toHaveBeenCalledTimes(1);
 await act(async()=>{await result.current.cancel();});
 expect(api.vendorJobHistory).toHaveBeenCalledTimes(2);
});
it.each(["expired", "invalid_input"])("retires missing prepared evidence on refresh (%s) without vendor actions",async code=>{
 const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));});
 api.vendorJobStatus.mockRejectedValueOnce({code});
 await act(async()=>{await result.current.refreshHistory();});
 expect(result.current.error).toMatch(/Refresh inventory and review again/);
 await act(async()=>{await result.current.confirm();await result.current.prepare(id(3),id(2));await result.current.cancel();});
 expect(api.prepareVendorJob).toHaveBeenCalledTimes(1);expect(api.confirmVendorJob).not.toHaveBeenCalled();expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it.each(["history", "status"])("recovers polling errors from terminal %s evidence without replay",async source=>{
 vi.useFakeTimers();api.confirmVendorJob.mockResolvedValueOnce(job("launching"));
 const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();});
 api.vendorJobStatus.mockRejectedValueOnce(new Error("offline"));
 await act(async()=>{await vi.advanceTimersByTimeAsync(750);});expect(result.current.error).not.toBeNull();
 if(source==="history")api.vendorJobHistory.mockResolvedValueOnce({records:[job("completed")],nextCursor:null});
 await act(async()=>{await result.current.refreshHistory();});
 expect(result.current.job?.state).toBe("completed");expect(result.current.error).toBeNull();
 const calls=api.vendorJobStatus.mock.calls.length;
 await act(async()=>{await vi.advanceTimersByTimeAsync(3000);await result.current.confirm();});
 expect(api.vendorJobStatus).toHaveBeenCalledTimes(calls);expect(api.vendorJobHistory).toHaveBeenCalledTimes(2);
 expect(api.prepareVendorJob).toHaveBeenCalledTimes(1);expect(api.confirmVendorJob).toHaveBeenCalledTimes(1);expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it("keeps missing submitted outcomes uncertain and recovers on later evidence",async()=>{
 vi.useFakeTimers();const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();});
 api.vendorJobStatus.mockRejectedValueOnce({code:"invalid_input"});
 await act(async()=>{await result.current.refreshHistory();});
 expect(result.current.reviewUnavailable).toBe(true);expect(result.current.error).toMatch(/outcome is unknown/);
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();await result.current.cancel();});
 api.vendorJobHistory.mockResolvedValueOnce({records:[job("completed")],nextCursor:null});
 await act(async()=>{await result.current.refreshHistory();});
 expect(result.current.job?.state).toBe("completed");expect(result.current.reviewUnavailable).toBe(false);expect(result.current.error).toBeNull();
 expect(api.prepareVendorJob).toHaveBeenCalledTimes(1);expect(api.confirmVendorJob).toHaveBeenCalledTimes(1);expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it.each(["resolve", "reject"])("ignores a late poll %s after refreshing terminal truth",async finish=>{
 vi.useFakeTimers();const late=deferred<VendorJob>();api.vendorJobStatus.mockReturnValueOnce(late.promise);
 const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();await vi.advanceTimersByTimeAsync(750);});
 api.vendorJobHistory.mockResolvedValueOnce({records:[job("completed")],nextCursor:null});
 await act(async()=>{await result.current.refreshHistory();});
 await act(async()=>{if(finish==="resolve")late.resolve(job("launching"));else late.reject(new Error("old failure"));});
 expect(result.current.job?.state).toBe("completed");expect(result.current.error).toBeNull();
 await act(async()=>{await vi.advanceTimersByTimeAsync(3000);});expect(api.vendorJobStatus).toHaveBeenCalledTimes(1);
});
it.each(["history", "status"])("ignores late selected %s responses after scope changes",async source=>{
 vi.useFakeTimers();const history=deferred<VendorHistoryPage>();const status=deferred<VendorJob>();
 const {result,rerender}=renderHook(({scope})=>useVendorJobs(scope),{initialProps:{scope:"one"}});
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();});
 if(source==="history")api.vendorJobHistory.mockReturnValueOnce(history.promise);else api.vendorJobStatus.mockReturnValueOnce(status.promise);
 await act(async()=>{void result.current.refreshHistory();});
 rerender({scope:"two"});
 await act(async()=>{history.resolve({records:[job("completed")],nextCursor:null});status.resolve(job("completed"));});
 expect(result.current.job).toBeNull();expect(result.current.error).toBeNull();expect(result.current.reviewUnavailable).toBe(false);
 expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it("coalesces refresh requests including their bounded selected status read",async()=>{
 const status=deferred<VendorJob>();const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));});
 api.vendorJobStatus.mockReturnValueOnce(status.promise);
 await act(async()=>{void result.current.refreshHistory();});
 let first!:Promise<void>;let second!:Promise<void>;
 act(()=>{first=result.current.refreshHistory();second=result.current.refreshHistory();});
 expect(first).toBe(second);expect(api.vendorJobHistory).toHaveBeenCalledTimes(2);expect(api.vendorJobStatus).toHaveBeenCalledTimes(1);
 await act(async()=>{status.resolve(job("cancelledBeforeLaunch"));await first;});
 expect(result.current.job?.state).toBe("cancelledBeforeLaunch");expect(result.current.error).toMatch(/Refresh inventory and review again/);
 expect(api.prepareVendorJob).toHaveBeenCalledTimes(1);expect(api.confirmVendorJob).not.toHaveBeenCalled();expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it("does not let a late refresh overwrite confirmation in the same scope",async()=>{
 const late=deferred<VendorHistoryPage>();const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));});
 api.vendorJobHistory.mockReturnValueOnce(late.promise);
 act(()=>{void result.current.refreshHistory();});
 api.confirmVendorJob.mockResolvedValueOnce(job("completed"));
 await act(async()=>{await result.current.confirm();late.resolve({records:[job()],nextCursor:null});});
 expect(result.current.job?.state).toBe("completed");expect(result.current.error).toBeNull();
 expect(api.vendorJobStatus).not.toHaveBeenCalled();expect(api.confirmVendorJob).toHaveBeenCalledTimes(1);
});
it("rejects mismatched program evidence and cannot revive a retired review",async()=>{
 const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));});
 api.vendorJobHistory.mockResolvedValueOnce({records:[{...job("completed"),programId:id(99)}],nextCursor:null});
 api.vendorJobStatus.mockResolvedValueOnce({...job("completed"),programId:id(99)});
 await act(async()=>{await result.current.refreshHistory();});
 expect(api.vendorJobStatus).toHaveBeenCalledWith(id(1));expect(result.current.reviewUnavailable).toBe(true);
 api.vendorJobHistory.mockResolvedValueOnce({records:[job()],nextCursor:null});
 await act(async()=>{await result.current.refreshHistory();await result.current.confirm();});
 expect(result.current.reviewUnavailable).toBe(true);expect(api.confirmVendorJob).not.toHaveBeenCalled();expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it("resumes serial polling after recovering still-pending evidence",async()=>{
 vi.useFakeTimers();const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));await result.current.confirm();});
 api.vendorJobStatus.mockRejectedValueOnce(new Error("offline"));
 await act(async()=>{await vi.advanceTimersByTimeAsync(750);});
 api.vendorJobHistory.mockResolvedValueOnce({records:[job("launching")],nextCursor:null});
 await act(async()=>{await result.current.refreshHistory();});
 expect(result.current.job?.state).toBe("launching");expect(result.current.error).toBeNull();
 await act(async()=>{await vi.advanceTimersByTimeAsync(750);});
 expect(result.current.job?.state).toBe("completed");expect(api.vendorJobStatus).toHaveBeenCalledTimes(2);
 expect(api.prepareVendorJob).toHaveBeenCalledTimes(1);expect(api.confirmVendorJob).toHaveBeenCalledTimes(1);expect(api.cancelVendorJob).not.toHaveBeenCalled();
});
it("rejects a mismatched confirmation response and does not double submit",async()=>{
 const pending=deferred<VendorJob>();api.confirmVendorJob.mockReturnValueOnce(pending.promise);
 const {result}=renderHook(()=>useVendorJobs("one"));
 await act(async()=>{await result.current.prepare(id(3),id(2));});
 act(()=>{void result.current.confirm();void result.current.confirm();});
 await act(async()=>pending.resolve({...job("completed"),jobId:id(99)}));
 expect(api.confirmVendorJob).toHaveBeenCalledTimes(1);expect(result.current.job?.state).toBe("awaitingConfirmation");expect(result.current.error).not.toBeNull();
});
