import type { Catalog } from "../../shared/i18n/catalog";

const en = {
  eyebrow: "Personal-file cleanup",
  title: "Duplicate files",
  intro: "Review independently verified copies. Nothing is selected automatically.",
  chooseFolder: "Choose folder",
  selectedFolder: (path: string) => `Selected folder: ${path}`,
  chooseAgain: "Choose a folder again to scan. Folder authorizations are single-use.",
  filters: "Scan filters",
  minimum: "Minimum size (MiB)",
  maximum: "Maximum size (MiB, blank for no maximum)",
  extensions: "Extensions (comma-separated, blank for all)",
  depth: "Scan depth",
  invalid: "Enter depth 0–64, valid minimum/maximum sizes, and at most 64 alphanumeric extensions.",
  scan: "Scan for duplicates",
  resultsNote:
    "Results are bounded and may be incomplete. Hard links count as one independent copy. Native checks revalidate content and the retained copy before cleanup.",
  keeperNote:
    "Changing groups clears selection. The first member on the first page is reserved as an unselectable keeper across all member pages.",
  backToGroups: "Back to groups (clear selection)",
  independentCopies: (count: number) => `${count} independent copies`,
  groupSummary: (members: number, bytesPerCopy: string) => `${members} members · ${bytesPerCopy} per copy`,
  groupRow: (copies: number, members: number, bytesPerCopy: string) =>
    `${copies} independent copies · ${members} members · ${bytesPerCopy} per copy`,
  waitingKeeper: "Waiting for the first-page keeper. Selection is disabled.",
  membersLabel: "Duplicate members",
  groupsLabel: "Duplicate groups",
  reviewGroup: "Review group",
  logical: (bytes: string) => `${bytes} logical`,
  keeper: " · Keeper (retained)",
  noMatches: "No matches on this page.",
  selectionStatus: (count: number, max: number) =>
    `${count} selected (maximum ${max}). App recovery keeps copies on the same volume for undo and does not free disk space. Permanent deletion is nonundoable and requires a separate Windows confirmation.`,
  clearSelection: "Clear selection",
};

const es419 = {
  eyebrow: "Limpieza de archivos personales",
  title: "Archivos duplicados",
  intro: "Revisa copias verificadas de forma independiente. No se selecciona nada automáticamente.",
  chooseFolder: "Elegir carpeta",
  selectedFolder: (path: string) => `Carpeta seleccionada: ${path}`,
  chooseAgain: "Elige una carpeta de nuevo para analizar. Las autorizaciones de carpeta son de un solo uso.",
  filters: "Filtros de análisis",
  minimum: "Tamaño mínimo (MiB)",
  maximum: "Tamaño máximo (MiB, vacío para no tener máximo)",
  extensions: "Extensiones (separadas por comas, vacío para todas)",
  depth: "Profundidad de análisis",
  invalid:
    "Ingresa una profundidad de 0 a 64, tamaños mínimo y máximo válidos y como máximo 64 extensiones alfanuméricas.",
  scan: "Buscar duplicados",
  resultsNote:
    "Los resultados son limitados y pueden estar incompletos. Los vínculos físicos cuentan como una sola copia independiente. Las comprobaciones nativas vuelven a validar el contenido y la copia conservada antes de la limpieza.",
  keeperNote:
    "Cambiar de grupo borra la selección. El primer miembro de la primera página se reserva como copia conservada no seleccionable en todas las páginas de miembros.",
  backToGroups: "Volver a los grupos (borra la selección)",
  independentCopies: (count: number) =>
    `${count} ${count === 1 ? "copia independiente" : "copias independientes"}`,
  groupSummary: (members: number, bytesPerCopy: string) =>
    `${members} ${members === 1 ? "miembro" : "miembros"} · ${bytesPerCopy} por copia`,
  groupRow: (copies: number, members: number, bytesPerCopy: string) =>
    `${copies} ${copies === 1 ? "copia independiente" : "copias independientes"} · ${members} ${members === 1 ? "miembro" : "miembros"} · ${bytesPerCopy} por copia`,
  waitingKeeper: "Esperando la copia conservada de la primera página. La selección está deshabilitada.",
  membersLabel: "Miembros duplicados",
  groupsLabel: "Grupos de duplicados",
  reviewGroup: "Revisar grupo",
  logical: (bytes: string) => `${bytes} lógicos`,
  keeper: " · Copia conservada (se mantiene)",
  noMatches: "No hay coincidencias en esta página.",
  selectionStatus: (count: number, max: number) =>
    `${count} ${count === 1 ? "seleccionado" : "seleccionados"} (máximo ${max}). La recuperación de la app guarda las copias en el mismo volumen para poder deshacer y no libera espacio en disco. La eliminación permanente no se puede deshacer y requiere una confirmación aparte de Windows.`,
  clearSelection: "Borrar selección",
} satisfies Catalog<typeof en>;

export const duplicatesStrings = { en, es419 };
export type DuplicatesStrings = typeof en;
