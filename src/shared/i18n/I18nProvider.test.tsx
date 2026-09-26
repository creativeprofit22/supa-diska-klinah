// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { formatBytes, formatDateTime, formatNumber, selectPlural } from "../format";
import type { Catalog } from "./catalog";
import { I18nProvider, useFormat, useI18n, useStrings } from "./I18nProvider";

const en = { title: "Files", count: (n: string) => `${n} files` };
const es419 = { title: "Archivos", count: (n: string) => `${n} archivos` } satisfies Catalog<typeof en>;
const strings = { en, es419 };

function Probe() {
  const t = useStrings(strings);
  const fmt = useFormat();
  const { setPreference } = useI18n();
  return <div>
    <h1>{t.title}</h1>
    <p>{t.count(fmt.number(1234567))}</p>
    <button type="button" onClick={() => setPreference("en")}>force English</button>
  </div>;
}

afterEach(cleanup);

describe("I18nProvider", () => {
  it("renders English without a provider", () => {
    render(<Probe />);
    expect(screen.getByRole("heading").textContent).toBe("Files");
    expect(screen.getByText("1,234,567 files")).toBeTruthy();
  });

  it("follows a Spanish Windows language and sets the document language", () => {
    render(<I18nProvider languages={["es-MX"]}><Probe /></I18nProvider>);
    expect(screen.getByRole("heading").textContent).toBe("Archivos");
    expect(screen.getByText(/archivos$/).textContent).toBe(`${formatNumber(1234567, "es-419")} archivos`);
    expect(document.documentElement.lang).toBe("es-419");
  });

  it("lets the saved preference override the system language", () => {
    render(<I18nProvider languages={["es-MX"]}><Probe /></I18nProvider>);
    act(() => screen.getByRole("button").click());
    expect(screen.getByRole("heading").textContent).toBe("Files");
    expect(document.documentElement.lang).toBe("en");
  });
});

describe("formatters", () => {
  it("formats bytes per locale", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1536, "es-419")).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 ** 5)).toBe("5,120 TB");
  });

  it("selects CLDR plural forms", () => {
    const forms = { one: "archivo", other: "archivos", many: "de archivos" };
    expect(selectPlural(1, forms, "es-419")).toBe("archivo");
    expect(selectPlural(2, forms, "es-419")).toBe("archivos");
    expect(selectPlural(0, { one: "file", other: "files" })).toBe("files");
  });

  it("returns null for out-of-range timestamps", () => {
    expect(formatDateTime(Number.MAX_SAFE_INTEGER)).toBeNull();
    expect(formatDateTime(0, "es-419")).toMatch(/1969|1970/);
  });
});
