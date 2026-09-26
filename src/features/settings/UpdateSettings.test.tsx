// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppSettings } from "../../shared/app-settings/api";
import { AppSettingsProvider } from "../../shared/app-settings/AppSettingsProvider";
import { parseUpdateStatus, type UpdateApi, type UpdateResult, type UpdateStatus } from "../../shared/app-settings/updateApi";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { UpdateSettings } from "./UpdateSettings";

const base: UpdateStatus = { currentVersion: "0.1.0", configured: true, update: { state: "idle" } };
const ok = (update: UpdateStatus["update"], overrides: Partial<UpdateStatus> = {}): UpdateResult => ({ ok: true, value: { ...base, ...overrides, update } });

function renderWith(api: Partial<UpdateApi>, settings: Partial<AppSettings> = {}, languages = ["en-US"]) {
  const full: UpdateApi = {
    status: vi.fn(async () => ok({ state: "idle" })),
    check: vi.fn(async () => ok({ state: "available", version: "0.2.0", size: 5 * 1024 * 1024 })),
    download: vi.fn(async () => ok({ state: "verified", version: "0.2.0" })),
    install: vi.fn(async () => ok({ state: "launched", version: "0.2.0" })),
    discard: vi.fn(async () => ok({ state: "idle" })),
    acknowledgeRecovery: vi.fn(async () => ok({ state: "idle" })),
    ...api,
  };
  const saved: AppSettings = { schemaVersion: 1, language: "system", updateCheck: false, ...settings };
  const loader = {
    get: vi.fn(async () => ({ ok: true as const, value: saved })),
    set: vi.fn(async (next: AppSettings) => ({ ok: true as const, value: next })),
  };
  render(
    <I18nProvider languages={languages}>
      <AppSettingsProvider loader={loader}><UpdateSettings api={full} /></AppSettingsProvider>
    </I18nProvider>,
  );
  return { api: full, loader };
}

afterEach(cleanup);

describe("UpdateSettings", () => {
  it("is off by default and offers no network action until enabled", async () => {
    const { api, loader } = renderWith({});
    const toggle = await screen.findByRole("checkbox", { name: /Allow checking for updates/ });
    await waitFor(() => expect((toggle as HTMLInputElement).disabled).toBe(false));
    expect((toggle as HTMLInputElement).checked).toBe(false);
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
    expect(api.check).not.toHaveBeenCalled();
    fireEvent.click(toggle);
    await waitFor(() => expect(loader.set).toHaveBeenCalledWith(expect.objectContaining({ updateCheck: true })));
    expect(await screen.findByRole("button", { name: "Check for updates" })).toBeTruthy();
  });

  it("walks check → download → install with explicit steps", async () => {
    const { api } = renderWith({}, { updateCheck: true });
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText("Version 0.2.0 is available (5 MB).")).toBeTruthy();
    expect(api.download).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Download and verify" }));
    expect(await screen.findByText("Version 0.2.0 is downloaded and verified.")).toBeTruthy();
    expect(api.install).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Install update…" }));
    expect(await screen.findByText("The installer for version 0.2.0 is running.")).toBeTruthy();
  });

  it("explains an interrupted install and offers retry or discard", async () => {
    const { api } = renderWith({ status: vi.fn(async () => ok({ state: "interrupted", version: "0.2.0" })) });
    expect((await screen.findByRole("alert")).textContent).toMatch(/didn't finish/);
    expect(screen.getByRole("button", { name: "Try the installation again…" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Discard download" }));
    await waitFor(() => expect(api.discard).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("maps typed errors to localized messages", async () => {
    renderWith({ check: vi.fn(async (): Promise<UpdateResult> => ({ ok: false, error: "integrityFailed" })) }, { updateCheck: true }, ["es-MX"]);
    fireEvent.click(await screen.findByRole("button", { name: "Buscar actualizaciones" }));
    expect((await screen.findByRole("alert")).textContent).toMatch(/no coincidía con la actualización firmada/);
  });

  it("shows a previous update being checked and refreshes once startup recovery is done", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const status = vi.fn()
        .mockResolvedValueOnce(ok({ state: "recovering" }, { recoveryPending: true }))
        .mockResolvedValueOnce(ok({ state: "recovering" }, { recoveryPending: true }))
        .mockResolvedValue(ok({ state: "verified", version: "0.2.0" }));
      renderWith({ status }, { updateCheck: true });
      expect(await screen.findByText("Checking a previous update…")).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
      await vi.advanceTimersByTimeAsync(1000);
      expect(status).toHaveBeenCalledTimes(2);
      await vi.advanceTimersByTimeAsync(1000);
      expect(await screen.findByText("Version 0.2.0 is downloaded and verified.")).toBeTruthy();
      expect(screen.queryByText("Checking a previous update…")).toBeNull();
      await vi.advanceTimersByTimeAsync(5000);
      expect(status).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops asking about recovery after unmount", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const status = vi.fn(async () => ok({ state: "recovering" }, { recoveryPending: true }));
      renderWith({ status }, {}, ["es-MX"]);
      expect(await screen.findByText("Revisando una actualización anterior…")).toBeTruthy();
      cleanup();
      await vi.advanceTimersByTimeAsync(5000);
      expect(status).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("says when the build has no update key", async () => {
    renderWith({ status: vi.fn(async () => ok({ state: "idle" }, { configured: false })) }, { updateCheck: true });
    expect(await screen.findByText("Updates are not available in this build.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
  });
});

describe("parseUpdateStatus", () => {
  it("rejects malformed backend data", () => {
    expect(parseUpdateStatus(base)).toEqual(base);
    const recovering = { ...base, update: { state: "recovering" }, recoveryPending: true };
    expect(parseUpdateStatus(recovering)).toEqual(recovering);
    expect(parseUpdateStatus({ ...base, update: { state: "available", version: "0.2.0", size: -1 } })).toBeNull();
    expect(parseUpdateStatus({ ...base, update: { state: "verified", version: "../x" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, update: { state: "hacked" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, currentVersion: 1 })).toBeNull();
  });

  it("accepts only well-formed startup recovery notices", () => {
    const updated = { ...base, recovery: { outcome: "updated", version: "0.2.0" } };
    expect(parseUpdateStatus(updated)).toEqual(updated);
    const interrupted = { ...base, recovery: { outcome: "interrupted", version: "0.2.0" } };
    expect(parseUpdateStatus(interrupted)).toEqual(interrupted);
    const discarded = { ...base, recovery: { outcome: "discarded" } };
    expect(parseUpdateStatus(discarded)).toEqual(discarded);
    expect(parseUpdateStatus({ ...base, recoveryPending: true })).toEqual({ ...base, recoveryPending: true });

    expect(parseUpdateStatus({ ...base, recovery: { outcome: "clean" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, recovery: { outcome: "updated" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, recovery: { outcome: "updated", version: "0.2" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, recovery: { outcome: "interrupted", version: "1234567890.0.0" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, recovery: { outcome: "discarded", version: "0.2.0" } })).toBeNull();
    expect(parseUpdateStatus({ ...base, recovery: "updated" })).toBeNull();
    expect(parseUpdateStatus({ ...base, recovery: null })).toBeNull();
    expect(parseUpdateStatus({ ...base, recoveryPending: false })).toBeNull();
  });
});
