// @vitest-environment jsdom
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { AppShell } from "../../shared/layout/AppShell";
import { type DriveInventory } from "./api";
import { DriveInventoryPage } from "./DriveInventoryPage";
import { drivesRoute } from "./route";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const empty: DriveInventory = { drives: [], partial: false, warnings: [] };
const populated: DriveInventory = {
  ...empty,
  drives: [{ driveId: "opaque-id", label: "Windows", filesystem: "NTFS", system: true,
    totalBytes: 1024 ** 4, usedBytes: 1024 ** 3, freeBytes: 1024 ** 4 - 1024 ** 3 }],
};
function deferred() {
  let resolve!: (value: DriveInventory) => void;
  const promise = new Promise<DriveInventory>((done) => { resolve = done; });
  return { promise, resolve };
}
afterEach(() => { cleanup(); invoke.mockReset(); });

it("loads through the real hook/API and coalesces StrictMode reads without exposing inputs", async () => {
  const pending = deferred();
  invoke.mockReturnValue(pending.promise);
  render(<StrictMode><DriveInventoryPage /></StrictMode>);
  expect(screen.getByRole("status").textContent).toContain("Reading fixed drives");
  expect((screen.getByRole("button", { name: "Refresh drives" }) as HTMLButtonElement).disabled).toBe(true);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("list_drive_inventory");
  await act(async () => pending.resolve(populated));
  expect(screen.getByRole("heading", { name: "Windows" })).toBeTruthy();
  expect(screen.getByText("System drive")).toBeTruthy();
  expect(screen.getByText("NTFS")).toBeTruthy();
  expect(screen.getByText("1 TB")).toBeTruthy();
  expect(screen.getByText("1 GB")).toBeTruthy();
  expect(screen.getByRole("status").textContent).toBe("1 fixed drive found.");
});

it("retains capacities and explicitly labels unknown system classification in incomplete results", async () => {
  invoke.mockResolvedValue({
    drives: [{ ...populated.drives[0]!, system: null }], partial: true,
    warnings: [{ drive: null, code: "inventory_partial" }],
  } satisfies DriveInventory);
  render(<DriveInventoryPage />);
  expect(await screen.findByText("System classification unavailable")).toBeTruthy();
  expect(screen.queryByText("System drive")).toBeNull();
  expect(screen.getByText("1 TB")).toBeTruthy();
  expect(screen.getByText("1 GB")).toBeTruthy();
  expect(screen.getByRole("status").textContent).toContain("Incomplete inventory");
  expect(screen.getByText("Windows returned only part of the drive inventory.")).toBeTruthy();
  expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Refresh drives"]);
});

it("renders an honest empty inventory", async () => {
  invoke.mockResolvedValue(empty);
  render(<DriveInventoryPage />);
  expect(await screen.findByRole("heading", { name: "No fixed drives found" })).toBeTruthy();
  expect(screen.queryByRole("list", { name: "Fixed drives" })).toBeNull();
});

it.each([
  ["busy", "A drive inventory is already running."],
  ["timeout", "Windows took too long"],
  ["inventory_unavailable", "Drive information is unavailable."],
])("maps %s to a safe error and retries through loading", async (code, message) => {
  invoke.mockRejectedValueOnce({ code, message: "SECRET native diagnostic" });
  const next = deferred();
  invoke.mockReturnValueOnce(next.promise);
  render(<DriveInventoryPage />);
  expect((await screen.findByRole("alert")).textContent).toContain(message);
  expect(screen.queryByText(/SECRET/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("status").textContent).toContain("Reading fixed drives");
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  await act(async () => next.resolve(populated));
  expect(screen.getByRole("heading", { name: "Windows" })).toBeTruthy();
});

it("refreshes instead of leaving stale drives visible during a new read", async () => {
  const next = deferred();
  invoke.mockResolvedValueOnce(populated).mockReturnValueOnce(next.promise);
  render(<DriveInventoryPage />);
  await screen.findByRole("heading", { name: "Windows" });
  fireEvent.click(screen.getByRole("button", { name: "Refresh drives" }));
  expect(screen.queryByRole("heading", { name: "Windows" })).toBeNull();
  await act(async () => next.resolve(empty));
  expect(screen.getByRole("heading", { name: "No fixed drives found" })).toBeTruthy();
});

it.each([true, false])("distinguishes partial results from empty success (populated=%s)", async (hasDrives) => {
  invoke.mockResolvedValue({
    ...(hasDrives ? populated : empty), partial: true,
    warnings: [{ drive: "Z:\\", code: "drive_unavailable" }, { drive: null, code: "inventory_partial" }],
  } satisfies DriveInventory);
  render(<DriveInventoryPage />);
  expect(await screen.findByRole("heading", { name: "Some drive information is unavailable" })).toBeTruthy();
  expect(screen.getByText(/Z:.*Windows could not read this drive/)).toBeTruthy();
  expect(screen.getByText("Windows returned only part of the drive inventory.")).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "No fixed drives found" })).toBeNull();
  expect(screen.getByRole("status").textContent).toContain("Incomplete inventory");
});

it("renders native labels as text and gives unlabelled drives a fallback", async () => {
  const drive = populated.drives[0]!;
  invoke.mockResolvedValue({ ...empty, drives: [
    { ...drive, label: "<img src=x onerror=alert(1)>" },
    { ...drive, driveId: "second-id", label: "  ", filesystem: "", system: false },
  ] });
  const { container } = render(<DriveInventoryPage />);
  expect(await screen.findByRole("heading", { name: "<img src=x onerror=alert(1)>" })).toBeTruthy();
  expect(container.querySelector("img")).toBeNull();
  expect(screen.getByRole("heading", { name: "Unlabelled drive 2" })).toBeTruthy();
  expect(screen.getByText("Not reported")).toBeTruthy();
});

it("does not update an unmounted view or issue another read during a pending remount", async () => {
  const pending = deferred();
  invoke.mockReturnValue(pending.promise);
  const view = render(<DriveInventoryPage />);
  view.unmount();
  render(<DriveInventoryPage />);
  await act(async () => pending.resolve(empty));
  expect(screen.getByRole("heading", { name: "No fixed drives found" })).toBeTruthy();
  expect(invoke).toHaveBeenCalledExactlyOnceWith("list_drive_inventory");
});

it("is reachable through the actual shell navigation and route", async () => {
  invoke.mockResolvedValue(populated);
  const router = createMemoryRouter([{ path: "/", element: <AppShell />, children: [drivesRoute] }]);
  render(<RouterProvider router={router} />);
  fireEvent.click(screen.getByRole("link", { name: "Drives" }));
  expect(await screen.findByRole("heading", { name: "Windows" })).toBeTruthy();
  expect(screen.getByRole("link", { name: "Drives" }).getAttribute("aria-current")).toBe("page");
  expect(document.title).toBe("Drives | Supa Diska Klinah");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("list_drive_inventory");
});
