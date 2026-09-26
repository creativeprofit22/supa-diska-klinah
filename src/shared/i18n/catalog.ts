import type { Locale } from "./locale";

/**
 * The shape every translation of a catalog must match: the same keys as the
 * English source, strings stay strings, and message functions keep their
 * parameters. Use `es419 satisfies Catalog<typeof en>` so a missing or extra
 * key is a compile error.
 */
export type Catalog<T> = {
  readonly [K in keyof T]: T[K] extends (...args: infer A) => string
    ? (...args: A) => string
    : T[K] extends string
      ? string
      : T[K] extends Readonly<Record<string, unknown>>
        ? Catalog<T[K]>
        : never;
};

/** One feature's strings in every shipped locale. */
export type Translations<T> = Readonly<{ en: T; es419: Catalog<T> }>;

export function pickCatalog<T>(translations: Translations<T>, locale: Locale): T {
  // Catalog<T> is structurally identical to T for well-formed catalogs.
  return (locale === "es-419" ? translations.es419 : translations.en) as T;
}
