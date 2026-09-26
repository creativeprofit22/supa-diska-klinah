// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../shared/i18n/I18nProvider";
import { StoragePlanReview } from "../shared/storage/StoragePlanReview";
import { appRoutes } from "./router";
import { respond, routePaths } from "./routeFixtures";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeAll(() => {
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.setAttribute("open", ""); } },
    close: { configurable: true, value(this: HTMLDialogElement) { this.removeAttribute("open"); } },
  });
  Element.prototype.scrollIntoView ??= () => undefined;
});
afterAll(() => {
  Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal");
  Reflect.deleteProperty(HTMLDialogElement.prototype, "close");
});
afterEach(() => { cleanup(); invoke.mockReset(); });

const FOCUSABLE = "a[href], button, input, select, textarea, summary, [tabindex]";

/** Sequential focus order as a browser computes it for tabindex <= 0 (DOM order). */
function tabStops(root: ParentNode = document.body): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((element) =>
    element.tabIndex >= 0 &&
    !(element as HTMLButtonElement).disabled &&
    !element.closest("[hidden], [inert], dialog:not([open]), fieldset:disabled") &&
    !(element.closest("details:not([open])") && element.tagName !== "SUMMARY"));
}

async function settle() {
  await act(async () => { for (let i = 0; i < 10; i++) await new Promise((done) => setTimeout(done, 0)); });
}

async function renderRoute(path: string) {
  invoke.mockImplementation(respond);
  const router = createMemoryRouter(appRoutes, { initialEntries: [path] });
  const view = render(<I18nProvider languages={["en-US"]}><RouterProvider router={router} /></I18nProvider>);
  await screen.findByRole("heading", { level: 1 });
  await settle();
  return view;
}

describe("app shell keyboard access", () => {
  it("makes the skip link the first Tab stop and moves focus to main", async () => {
    await renderRoute("/");
    const stops = tabStops();
    const skip = screen.getByRole("link", { name: "Skip to content" });
    expect(stops[0]).toBe(skip);
    const main = screen.getByRole("main");
    expect(main.tabIndex).toBe(-1);
    skip.focus();
    fireEvent.click(skip);
    expect(document.activeElement).toBe(main);
  });

  it("reaches every sidebar link by Tab in visual order", async () => {
    await renderRoute("/");
    const navLinks = within(screen.getByRole("navigation", { name: "Primary navigation" })).getAllByRole("link");
    const stops = tabStops();
    const positions = navLinks.map((link) => stops.indexOf(link));
    expect(positions.every((position) => position > 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((left, right) => left - right));
    // Nothing interleaves between the skip link and the navigation.
    expect(positions[0]).toBe(1);
    expect(navLinks.map((link) => link.getAttribute("href"))).toEqual(routePaths.filter((path) => !path.startsWith("/protection/")));
  });

  it.each(routePaths)("%s has no positive tabindex", async (path) => {
    const { container } = await renderRoute(path);
    const positive = [...container.querySelectorAll<HTMLElement>("[tabindex]")].filter((element) => element.tabIndex > 0);
    expect(positive.map((element) => element.outerHTML.slice(0, 120))).toEqual([]);
  });
});

function expectTrapped(dialog: HTMLElement) {
  const buttons = tabStops(dialog);
  expect(buttons.length).toBeGreaterThan(0);
  const first = buttons[0];
  const last = buttons[buttons.length - 1];
  last.focus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(document.activeElement).toBe(first);
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(last);
}

describe("modal dialogs", () => {
  it("cleanup confirmation traps Tab, closes on Escape and returns focus to its trigger", async () => {
    await renderRoute("/cleanup");
    invoke.mockImplementation((command: string, bytes?: unknown) => command === "create_cleanup_plan"
      ? Promise.resolve({ planId: "c".repeat(32), disposition: "recycleBin", selectedCount: 1, selectedBytes: 1024 })
      : respond(command, bytes));
    fireEvent.click(screen.getByRole("button", { name: "Select all" }));
    const trigger = screen.getByRole("button", { name: "Move to Recycle Bin" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog");
    expect(dialog.contains(document.activeElement)).toBe(true);
    expectTrapped(dialog);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("storage plan review traps Tab, closes on Escape and returns focus to its trigger", async () => {
    const summary = { planId: "c".repeat(32), disposition: "recycleBin" as const, selectedCount: 1, selectedBytes: 512 };
    const createPlan = vi.fn().mockResolvedValue(summary);
    const actions = {
      executeCleanupPlan: vi.fn(), executePermanentCleanupPlan: vi.fn(), undoCleanup: vi.fn(),
      cleanupHistory: vi.fn().mockResolvedValue({ records: [], nextCursor: null }),
    };
    render(<StoragePlanReview selection={{ module: "largeFiles", snapshotId: "a".repeat(32), candidateIds: ["b".repeat(32)] }} createPlan={createPlan} actions={actions} onExecuted={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: "Review: Move to Recycle Bin" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog");
    expect(dialog.contains(document.activeElement)).toBe(true);
    expectTrapped(dialog);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(dialog.hasAttribute("open")).toBe(false);
    expect(document.activeElement).toBe(trigger);
    expect(actions.executeCleanupPlan).not.toHaveBeenCalled();
  });
});
