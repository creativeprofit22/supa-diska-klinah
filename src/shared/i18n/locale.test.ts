import { describe, expect, it } from "vitest";
import { isLanguagePreference, matchLocale, resolveLocale } from "./locale";

describe("resolveLocale", () => {
  it.each([
    ["system", ["es-MX"], "es-419"],
    ["system", ["es-AR", "en-US"], "es-419"],
    ["system", ["es-CO"], "es-419"],
    ["system", ["es-ES"], "es-419"],
    ["system", ["es"], "es-419"],
    ["system", ["es-419"], "es-419"],
    ["system", ["ES_mx"], "es-419"],
    ["system", ["en-GB"], "en"],
    ["system", ["fr-FR", "es-MX"], "es-419"],
    ["system", ["fr-FR", "de-DE"], "en"],
    ["system", [], "en"],
    ["system", [""], "en"],
    ["en", ["es-MX"], "en"],
    ["es-419", ["en-US"], "es-419"],
  ] as const)("%s with %j resolves to %s", (preference, languages, expected) => {
    expect(resolveLocale(preference, languages)).toBe(expected);
  });

  it("does not treat look-alike primary tags as Spanish", () => {
    expect(matchLocale("est")).toBeNull();
    expect(matchLocale("eng")).toBeNull();
  });

  it("validates stored preferences", () => {
    expect(isLanguagePreference("system")).toBe(true);
    expect(isLanguagePreference("es-419")).toBe(true);
    expect(isLanguagePreference("es-MX")).toBe(false);
    expect(isLanguagePreference(1)).toBe(false);
  });
});
