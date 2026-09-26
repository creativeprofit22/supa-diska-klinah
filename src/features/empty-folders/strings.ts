import type { Catalog } from "../../shared/i18n/catalog";

const en = {
  eyebrow: "Personal-file cleanup",
  title: "Empty folders",
  intro: "Explicitly select empty directories for review. Nothing is selected automatically.",
  rootRetained:
    "The chosen root is always retained. Blocked, non-empty, hidden, protected and incomplete parents are absent from results.",
  permanentNote:
    "Permanent deletion only, with no undo. Exact empty-only atomic directory removal cannot safely quarantine a directory that gains a child. A new child blocks removal. No app recovery or Windows Recycle Bin is offered.",
  chooseFolder: "Choose folder",
  selectedFolder: (path: string) => `Selected folder: ${path}`,
  chooseAgain: "Choose a folder again to scan. Folder authorizations are single-use.",
  filters: "Scan filters",
  depth: "Scan depth",
  depthInvalid: "Enter a whole depth from 0 to 64.",
  scan: "Scan for empty folders",
  resultsNote:
    "Results are bounded. Only complete empty subtrees are eligible. Permanent deletion requires separate Windows confirmation.",
  selectedCount: (count: number, max: number) => `${count} selected (maximum ${max}).`,
  clearSelection: "Clear selection",
  resultsLabel: "Empty folder results",
  rowDetail: (depth: number, descendants: number) => `Depth ${depth} · ${descendants} descendant directories`,
  noResults: "No empty folders on this page.",
};

const es419 = {
  eyebrow: "Limpieza de archivos personales",
  title: "Carpetas vacías",
  intro: "Selecciona de forma explícita las carpetas vacías que quieres revisar. No se selecciona nada automáticamente.",
  rootRetained:
    "La carpeta raíz elegida siempre se conserva. Las carpetas principales bloqueadas, no vacías, ocultas, protegidas o incompletas no aparecen en los resultados.",
  permanentNote:
    "Solo eliminación permanente, sin opción de deshacer. La eliminación atómica exacta de carpetas solo vacías no puede enviar de forma segura a recuperación de la app una carpeta que recibe un elemento nuevo. Un elemento nuevo bloquea la eliminación. No se ofrece recuperación de la app ni la Papelera de reciclaje de Windows.",
  chooseFolder: "Elegir carpeta",
  selectedFolder: (path: string) => `Carpeta seleccionada: ${path}`,
  chooseAgain: "Elige una carpeta de nuevo para analizar. Las autorizaciones de carpeta son de un solo uso.",
  filters: "Filtros de análisis",
  depth: "Profundidad de análisis",
  depthInvalid: "Ingresa una profundidad entera de 0 a 64.",
  scan: "Buscar carpetas vacías",
  resultsNote:
    "Los resultados son limitados. Solo son aptos los subárboles vacíos completos. La eliminación permanente requiere una confirmación aparte de Windows.",
  selectedCount: (count: number, max: number) =>
    `${count} ${count === 1 ? "seleccionado" : "seleccionados"} (máximo ${max}).`,
  clearSelection: "Borrar selección",
  resultsLabel: "Resultados de carpetas vacías",
  rowDetail: (depth: number, descendants: number) =>
    `Profundidad ${depth} · ${descendants} ${descendants === 1 ? "carpeta descendiente" : "carpetas descendientes"}`,
  noResults: "No hay carpetas vacías en esta página.",
} satisfies Catalog<typeof en>;

export const emptyFoldersStrings = { en, es419 };
export type EmptyFoldersStrings = typeof en;
