import type { Catalog } from "../../shared/i18n/catalog";

const en = {
  documentTitle: "Drives | Supa Diska Klinah",
  kicker: "Read-only inventory",
  heading: "Fixed drives",
  intro: "Review local drive capacity. No files are scanned, changed, or removed.",
  tryAgain: "Try again",
  refresh: "Refresh drives",
  diagnostics:
    "Capacity is reported by Windows for your account, including any quota limits. Removable, network, and optical drives are not included.",
  reading: "Reading fixed drives from Windows…",
  incomplete: (_count: number, formatted: string) => `Incomplete inventory: ${formatted} fixed drives available.`,
  found: (count: number, formatted: string) => `${formatted} fixed ${count === 1 ? "drive" : "drives"} found.`,
  readError: "Could not read drive information",
  partialHeading: "Some drive information is unavailable",
  partialBody: "Missing information is not zero capacity. Refresh to try again.",
  driveUnavailable: (drive: string | null) => `${drive ?? "A fixed drive"}: Windows could not read this drive.`,
  inventoryPartial: "Windows returned only part of the drive inventory.",
  emptyHeading: "No fixed drives found",
  emptyBody: "Windows reported no eligible local fixed drives. Refresh to check again.",
  listLabel: "Fixed drives",
  unlabelled: "Unlabelled drive",
  systemDrive: "System drive",
  systemUnknown: "System classification unavailable",
  fileSystem: "File system",
  notReported: "Not reported",
  totalCapacity: "Total capacity",
  usedSpace: "Used space",
  availableSpace: "Available space",
  errors: {
    busy: "A drive inventory is already running. Try again shortly.",
    timeout: "Windows took too long to report drive information. Try again.",
    unavailable: "Drive information is unavailable. Open the Windows app and try again.",
  },
};

const es419 = {
  documentTitle: "Unidades | Supa Diska Klinah",
  kicker: "Inventario de solo lectura",
  heading: "Unidades fijas",
  intro: "Revisa la capacidad de las unidades locales. No se analiza, cambia ni elimina ningún archivo.",
  tryAgain: "Intentar de nuevo",
  refresh: "Actualizar unidades",
  diagnostics:
    "Windows informa la capacidad para tu cuenta, incluidos los límites de cuota. No se incluyen unidades extraíbles, de red ni ópticas.",
  reading: "Leyendo las unidades fijas desde Windows…",
  incomplete: (count: number, formatted: string) =>
    count === 1
      ? `Inventario incompleto: ${formatted} unidad fija disponible.`
      : `Inventario incompleto: ${formatted} unidades fijas disponibles.`,
  found: (count: number, formatted: string) =>
    count === 1 ? `${formatted} unidad fija encontrada.` : `${formatted} unidades fijas encontradas.`,
  readError: "No se pudo leer la información de las unidades",
  partialHeading: "Parte de la información de las unidades no está disponible",
  partialBody: "La información faltante no significa capacidad cero. Actualiza para intentarlo de nuevo.",
  driveUnavailable: (drive: string | null) => `${drive ?? "Una unidad fija"}: Windows no pudo leer esta unidad.`,
  inventoryPartial: "Windows devolvió solo una parte del inventario de unidades.",
  emptyHeading: "No se encontraron unidades fijas",
  emptyBody: "Windows no informó unidades fijas locales aptas. Actualiza para revisar de nuevo.",
  listLabel: "Unidades fijas",
  unlabelled: "Unidad sin etiqueta",
  systemDrive: "Unidad del sistema",
  systemUnknown: "Clasificación del sistema no disponible",
  fileSystem: "Sistema de archivos",
  notReported: "No informado",
  totalCapacity: "Capacidad total",
  usedSpace: "Espacio usado",
  availableSpace: "Espacio disponible",
  errors: {
    busy: "Ya hay un inventario de unidades en curso. Intenta de nuevo en un momento.",
    timeout: "Windows tardó demasiado en informar los datos de las unidades. Intenta de nuevo.",
    unavailable: "La información de las unidades no está disponible. Abre la app de Windows e intenta de nuevo.",
  },
} satisfies Catalog<typeof en>;

export const drivesStrings = { en, es419 };
export type DrivesStrings = typeof en;
