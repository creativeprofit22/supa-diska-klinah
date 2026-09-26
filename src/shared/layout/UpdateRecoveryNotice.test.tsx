// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { UpdateRecovery, UpdateResult, UpdateStatus } from "../app-settings/updateApi";
import { I18nProvider } from "../i18n/I18nProvider";
import { UpdateRecoveryNotice } from "./UpdateRecoveryNotice";

const base: UpdateStatus = { currentVersion: "0.2.0", configured: true, update: { state: "idle" } };
const ok = (overrides: Partial<UpdateStatus> = {}): UpdateResult => ({ ok: true, value: { ...base, ...overrides } });
const withRecovery = (recovery: UpdateRecovery): UpdateResult => ok({ recovery });

function renderNotice(status: () => Promise<UpdateResult>, languages = ["en-US"]) {
  const api = { status: vi.fn(status), acknowledgeRecovery: vi.fn(async () => ok()) };
  const router = createMemoryRouter([
    { path: "/", element: <UpdateRecoveryNotice api={api} /> },
    { path: "/settings", element: <p>Settings page</p> },
  ]);
  render(<I18nProvider languages={languages}><RouterProvider router={router} /></I18nProvider>);
  return { api, router };
}

afterEach(cleanup);

describe("UpdateRecoveryNotice", () => {
  it("announces a finished update politely and dismisses it once", async () => {
    const { api } = renderNotice(async () => withRecovery({ outcome: "updated", version: "0.2.0" }));
    const notice = await screen.findByRole("status");
    expect(notice.textContent).toContain("updated to version 0.2.0");
    expect(screen.queryByRole("link", { name: "Open Settings" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(api.acknowledgeRecovery).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("alerts about an interrupted update and links to retry/discard in Settings", async () => {
    const { router } = renderNotice(async () => withRecovery({ outcome: "interrupted", version: "0.3.0" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Update didn't finish");
    expect(alert.textContent).toContain("0.3.0");
    fireEvent.click(screen.getByRole("link", { name: "Open Settings" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/settings"));
  });

  it("explains a discarded installer in Latin American Spanish", async () => {
    renderNotice(async () => withRecovery({ outcome: "discarded" }), ["es-MX"]);
    const notice = await screen.findByRole("status");
    expect(notice.textContent).toContain("No se instaló nada");
    expect(screen.getByRole("link", { name: "Abrir Configuración" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Descartar aviso" })).toBeTruthy();
  });

  it("waits for background recovery to finish before deciding", async () => {
    const responses = [ok({ recoveryPending: true }), withRecovery({ outcome: "discarded" })];
    const { api } = renderNotice(async () => responses.shift() ?? ok());
    expect(await screen.findByRole("status", {}, { timeout: 2000 })).toBeTruthy();
    expect(api.status).toHaveBeenCalledTimes(2);
  });

  it("shows nothing for a clean start or an unavailable backend", async () => {
    const clean = renderNotice(async () => ok());
    await waitFor(() => expect(clean.api.status).toHaveBeenCalled());
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    cleanup();
    const failed = renderNotice(async () => ({ ok: false, error: "invalidResponse" }));
    await waitFor(() => expect(failed.api.status).toHaveBeenCalled());
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
