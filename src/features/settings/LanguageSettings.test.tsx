// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type AppSettings, parseAppSettings } from "../../shared/app-settings/api";
import { AppSettingsProvider } from "../../shared/app-settings/AppSettingsProvider";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { LanguageSettings } from "./LanguageSettings";

const saved: AppSettings = { schemaVersion: 1, language: "system", updateCheck: false };

function renderWith(get: () => Promise<unknown>, set: (next: AppSettings) => Promise<unknown>) {
  const loader = {
    get: vi.fn(async () => {
      const value = parseAppSettings(await get());
      return value ? { ok: true as const, value } : { ok: false as const, error: "failed" as const };
    }),
    set: vi.fn(async (next: AppSettings) => {
      try {
        const value = parseAppSettings(await set(next));
        return value ? { ok: true as const, value } : { ok: false as const, error: "invalidResponse" as const };
      } catch {
        return { ok: false as const, error: "failed" as const };
      }
    }),
  };
  render(
    <I18nProvider languages={["en-US"]}>
      <AppSettingsProvider loader={loader}><LanguageSettings /></AppSettingsProvider>
    </I18nProvider>,
  );
  return loader;
}

afterEach(cleanup);

describe("LanguageSettings", () => {
  it("switches the whole UI to Spanish and persists the choice", async () => {
    const loader = renderWith(async () => saved, async (next) => next);
    const select = await screen.findByLabelText(/Display language/);
    await waitFor(() => expect((select as HTMLSelectElement).disabled).toBe(false));
    fireEvent.change(select, { target: { value: "es-419" } });
    expect(await screen.findByRole("heading", { name: "Idioma" })).toBeTruthy();
    expect(loader.set).toHaveBeenCalledWith({ ...saved, language: "es-419" });
    // The provider sets <html lang> in an effect after the translated render commits.
    await waitFor(() => expect(document.documentElement.lang).toBe("es-419"));
  });

  it("applies a saved language at startup", async () => {
    renderWith(async () => ({ ...saved, language: "es-419" }), async (next) => next);
    expect(await screen.findByRole("heading", { name: "Idioma" })).toBeTruthy();
  });

  it("keeps the previous language and reports a failed save", async () => {
    renderWith(async () => saved, async () => { throw new Error("disk full"); });
    const select = await screen.findByLabelText(/Display language/);
    await waitFor(() => expect((select as HTMLSelectElement).disabled).toBe(false));
    fireEvent.change(select, { target: { value: "es-419" } });
    expect((await screen.findByRole("alert")).textContent).toMatch(/could not be saved/);
    expect(screen.getByRole("heading", { name: "Language" })).toBeTruthy();
  });

  it("names each language in itself", async () => {
    renderWith(async () => saved, async (next) => next);
    const options = (await screen.findAllByRole("option")).map((option) => option.textContent);
    expect(options).toEqual(["System (Windows)", "English", "Español (Latinoamérica)"]);
  });
});

describe("parseAppSettings", () => {
  it("rejects unknown fields, locales and types", () => {
    expect(parseAppSettings(saved)).toEqual(saved);
    expect(parseAppSettings({ ...saved, language: "es-MX" })).toBeNull();
    expect(parseAppSettings({ ...saved, updateCheck: "yes" })).toBeNull();
    expect(parseAppSettings({ ...saved, extra: 1 })).toBeNull();
    expect(parseAppSettings({ ...saved, schemaVersion: 2 })).toBeNull();
    expect(parseAppSettings(null)).toBeNull();
  });
});
