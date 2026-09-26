// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../shared/i18n/I18nProvider";
import { appRoutes } from "./router";
import { respond, routePaths } from "./routeFixtures";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeAll(() => {
  // jsdom lacks the modal dialog API and scrolling.
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

async function audit(container: HTMLElement): Promise<string[]> {
  const result = await axe.run(container, {
    resultTypes: ["violations"],
    // jsdom cannot compute colours; scripts/check-contrast.mjs covers contrast.
    rules: { "color-contrast": { enabled: false } },
  });
  return result.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id} (${violation.impact}): ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

// Guards against a vacuous audit: the same helper must report planted problems.
describe("axe audit sensitivity", () => {
  it("reports serious violations it is meant to catch", async () => {
    const { container } = render(
      <main>
        <h1>Probe</h1>
        <div aria-labelledby="missing">prohibited name on a generic element</div>
        <button type="button" />
        <img src="x.png" />
      </main>,
    );
    const ids = (await audit(container)).map((line) => line.split(" ")[0]);
    expect(ids).toEqual(expect.arrayContaining(["button-name", "image-alt"]));
  });
});

describe.each(["en-US", "es-MX"])("axe audit (%s)", (lang) => {
  it.each(routePaths)("%s has no serious or critical violations", async (path) => {
    invoke.mockImplementation(respond);
    const router = createMemoryRouter(appRoutes, { initialEntries: [path] });
    const { container } = render(<I18nProvider languages={[lang]}><RouterProvider router={router} /></I18nProvider>);
    await screen.findByRole("heading", { level: 1 });
    // Let mocked IPC settle so loaded content (or error states) is audited, not only spinners.
    await act(async () => { for (let i = 0; i < 10; i++) await new Promise((done) => setTimeout(done, 0)); });
    const violations = await audit(container);
    expect(violations, `${path} [${lang}]\n${violations.join("\n")}`).toEqual([]);
  });
});
