import type { Catalog } from "../i18n/catalog";

const en = {
  historyLoadFailed: "History could not be loaded. Try loading history again.",
};

const es419 = {
  historyLoadFailed: "No se pudo cargar el historial. Intenta cargarlo de nuevo.",
} satisfies Catalog<typeof en>;

export const cleanupStrings = { en, es419 };
export type CleanupStrings = typeof en;
