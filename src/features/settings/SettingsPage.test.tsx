// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./SettingsPage";
import { I18nProvider } from "../../shared/i18n/I18nProvider";

vi.mock("../build-artifacts/ArtifactBudgetSettings", () => ({
  ArtifactBudgetSettings: () => <div>Artifact budget settings</div>,
}));

vi.mock("./ScanSpeedSettings", () => ({
  ScanSpeedSettings: () => <div>Scan speed settings</div>,
}));

// Covered by LanguageSettings.test.tsx and UpdateSettings.test.tsx.
vi.mock("./LanguageSettings", () => ({
  LanguageSettings: () => <div>Language settings</div>,
}));

vi.mock("./UpdateSettings", () => ({
  UpdateSettings: () => <div>Update settings</div>,
}));

const { getAutoCleanupPolicy, setAutoCleanupPolicy } = vi.hoisted(() => ({
  getAutoCleanupPolicy: vi.fn(),
  setAutoCleanupPolicy: vi.fn(),
}));

vi.mock("./api/autoCleanupPolicy", () => ({
  getAutoCleanupPolicy,
  setAutoCleanupPolicy,
}));

const policy = { schemaVersion: 1, enabled: false, graceDays: 7 };

describe("SettingsPage automatic cleanup policy", () => {
  afterEach(() => {
    cleanup();
    getAutoCleanupPolicy.mockReset();
    setAutoCleanupPolicy.mockReset();
  });

  it("loads saved values and saves only explicit changes", async () => {
    getAutoCleanupPolicy.mockResolvedValue(policy);
    setAutoCleanupPolicy.mockResolvedValue({ ...policy, enabled: true, graceDays: 14 });

    render(<SettingsPage />);

    expect(screen.getByRole("status").textContent).toContain("Loading");
    const enabled = await screen.findByRole("checkbox", {
      name: /^Clean temporary caches at startup/,
    });
    const graceDays = screen.getByRole("combobox", { name: /^Recovery grace period/ });
    const save = screen.getByRole("button", { name: "Save changes" });

    expect((enabled as HTMLInputElement).checked).toBe(false);
    expect((graceDays as HTMLSelectElement).value).toBe("7");
    expect((save as HTMLButtonElement).disabled).toBe(true);
    expect(setAutoCleanupPolicy).not.toHaveBeenCalled();

    fireEvent.click(enabled);
    fireEvent.change(graceDays, { target: { value: "14" } });

    expect(setAutoCleanupPolicy).not.toHaveBeenCalled();
    expect((save as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(save);

    await waitFor(() => expect(setAutoCleanupPolicy).toHaveBeenCalledWith(true, 14));
    expect(await screen.findByText("Cleanup settings saved.")).toBeTruthy();
    expect((save as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows load failure and retries", async () => {
    getAutoCleanupPolicy.mockRejectedValueOnce(new Error("unavailable"));
    getAutoCleanupPolicy.mockResolvedValueOnce(policy);

    render(<SettingsPage />);

    expect((await screen.findByRole("alert")).textContent).toContain(
      "Cleanup settings could not be loaded.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    expect((await screen.findByRole("checkbox") as HTMLInputElement).checked).toBe(false);
    expect(getAutoCleanupPolicy).toHaveBeenCalledTimes(2);
  });

  it("keeps edited values when saving fails", async () => {
    getAutoCleanupPolicy.mockResolvedValue(policy);
    setAutoCleanupPolicy.mockRejectedValue(new Error("unavailable"));

    render(<SettingsPage />);

    const enabled = await screen.findByRole("checkbox");
    fireEvent.click(enabled);
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    expect((await screen.findByRole("alert")).textContent).toContain("changes remain unsaved");
    expect((enabled as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole("button", { name: "Save changes" }) as HTMLButtonElement).disabled).toBe(false);
  });
});

describe("Settings page localization", () => {
  afterEach(() => cleanup());
  it("renders Spanish copy for es-MX", async () => {
    getAutoCleanupPolicy.mockResolvedValue({ enabled: false, graceDays: 7 });
    render(<I18nProvider languages={["es-MX"]}><SettingsPage /></I18nProvider>);
    expect(screen.getByRole("heading", { name: "Configuración" })).toBeTruthy();
    expect(await screen.findByText("Limpiar cachés temporales al iniciar")).toBeTruthy();
    expect(screen.getByRole("option", { name: "7 días" })).toBeTruthy();
    expect(document.title).toBe("Configuración | Supa Diska Klinah");
  });
});
