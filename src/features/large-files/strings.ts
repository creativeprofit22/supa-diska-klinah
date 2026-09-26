import type { Catalog } from "../../shared/i18n/catalog";
import type { LargeFileFilter } from "./types";

const en = {
  extensionsPlaceholder: "pdf, zip",
  eyebrow: "Personal-file cleanup",
  heading: "Large files",
  intro: "Find large files in a chosen folder, then explicitly select files to review. Nothing is selected automatically.",
  choosingFolder: "Choosing folder…",
  chooseFolder: "Choose folder",
  selectedFolder: (path: string) => `Selected folder: ${path}`,
  chooseAgain: "Choose a folder again to scan. Folder authorizations are single-use.",
  filters: "Scan filters",
  minimumSize: "Minimum size (MiB)",
  maximumSize: "Maximum size (MiB, optional)",
  depth: "Scan depth",
  extensions: "Extensions",
  category: "Category",
  categories: {
    any: "Any category",
    documents: "Documents",
    images: "Images",
    audio: "Audio",
    video: "Video",
    archives: "Archives",
    other: "Other",
  } satisfies Record<LargeFileFilter["category"], string>,
  sortBy: "Sort by",
  sorts: {
    size: "Size",
    modified: "Modification time",
    path: "Path",
  } satisfies Record<LargeFileFilter["sort"], string>,
  descending: "Descending order",
  filterHelp:
    "Depth 0–64. Sizes must convert to safe whole-byte values; maximum must be at least minimum. Leave maximum blank for no maximum. Extensions: up to 64 comma- or space-separated bare alphanumeric names, at most 32 characters each; uppercase is normalized. Leave blank for all extensions.",
  invalidFilters: "Enter valid sizes, depth and extensions before scanning.",
  scan: "Scan for large files",
  partial:
    "Partial results: traversal or retention limits, protected, inaccessible or changing files may omit matches. These are not necessarily the globally largest files; narrow the scope and scan again.",
  bytesNote:
    "Logical and allocated bytes describe files, not reclaimed space. Hard links and backend safety checks can affect cleanup outcomes.",
  selection: (_count: number, selected: string, maximum: string) =>
    `${selected} selected (maximum ${maximum}). App recovery keeps files for undo on the same volume without freeing disk space. Windows Recycle Bin is not supported for these identity-checked actions. Permanent deletion requires a separate Windows confirmation.`,
  clearSelection: "Clear selection",
  noMatches: "No matching files on this page.",
  resultsLabel: "Large file results",
  unknownAllocation: "Unknown allocation",
  allocated: (bytes: string) => `${bytes} allocated`,
  fileRow: (logical: string, allocation: string, modified: string) => `${logical} logical · ${allocation} · ${modified}`,
  unknownModified: "Unknown modification time",
  readOnly: "Read-only result: not selectable.",
  notEligible: (reason: string) => `Not eligible: ${reason}`,
};

const es419 = {
  extensionsPlaceholder: "pdf, zip",
  eyebrow: "Limpieza de archivos personales",
  heading: "Archivos grandes",
  intro:
    "Busca archivos grandes en una carpeta que elijas y luego selecciona explícitamente los archivos para revisar. No se selecciona nada automáticamente.",
  choosingFolder: "Eligiendo carpeta…",
  chooseFolder: "Elegir carpeta",
  selectedFolder: (path: string) => `Carpeta seleccionada: ${path}`,
  chooseAgain: "Vuelve a elegir una carpeta para analizar. Las autorizaciones de carpeta son de un solo uso.",
  filters: "Filtros de análisis",
  minimumSize: "Tamaño mínimo (MiB)",
  maximumSize: "Tamaño máximo (MiB, opcional)",
  depth: "Profundidad del análisis",
  extensions: "Extensiones",
  category: "Categoría",
  categories: {
    any: "Cualquier categoría",
    documents: "Documentos",
    images: "Imágenes",
    audio: "Audio",
    video: "Video",
    archives: "Archivos comprimidos",
    other: "Otros",
  },
  sortBy: "Ordenar por",
  sorts: {
    size: "Tamaño",
    modified: "Fecha de modificación",
    path: "Ruta",
  },
  descending: "Orden descendente",
  filterHelp:
    "Profundidad 0–64. Los tamaños deben convertirse en valores seguros de bytes enteros; el máximo debe ser al menos igual al mínimo. Deja el máximo en blanco para no tener máximo. Extensiones: hasta 64 nombres alfanuméricos simples separados por comas o espacios, de 32 caracteres como máximo cada uno; las mayúsculas se normalizan. Déjalo en blanco para incluir todas las extensiones.",
  invalidFilters: "Escribe tamaños, profundidad y extensiones válidos antes de analizar.",
  scan: "Buscar archivos grandes",
  partial:
    "Resultados parciales: los límites de recorrido o retención y los archivos protegidos, inaccesibles o que están cambiando pueden omitir coincidencias. No son necesariamente los archivos más grandes de todo el disco; reduce el alcance y vuelve a analizar.",
  bytesNote:
    "Los bytes lógicos y asignados describen archivos, no espacio recuperado. Los vínculos físicos y las comprobaciones de seguridad del sistema pueden afectar el resultado de la limpieza.",
  selection: (count: number, selected: string, maximum: string) =>
    `${selected} ${count === 1 ? "seleccionado" : "seleccionados"} (máximo ${maximum}). La recuperación de la app guarda los archivos para deshacer en el mismo volumen sin liberar espacio en disco. La Papelera de reciclaje de Windows no es compatible con estas acciones verificadas por identidad. Eliminar permanentemente requiere una confirmación aparte de Windows.`,
  clearSelection: "Borrar selección",
  noMatches: "No hay archivos coincidentes en esta página.",
  resultsLabel: "Resultados de archivos grandes",
  unknownAllocation: "Asignación desconocida",
  allocated: (bytes: string) => `${bytes} asignado`,
  fileRow: (logical: string, allocation: string, modified: string) => `${logical} lógico · ${allocation} · ${modified}`,
  unknownModified: "Fecha de modificación desconocida",
  readOnly: "Resultado de solo lectura: no se puede seleccionar.",
  notEligible: (reason: string) => `No apto: ${reason}`,
} satisfies Catalog<typeof en>;

export const largeFilesStrings = { en, es419 };
export type LargeFilesStrings = typeof en;
