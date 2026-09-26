import type { Catalog } from "../../shared/i18n/catalog";

const en = {
  eyebrow: "Read-only storage analysis",
  heading: "Disk analyzer",
  intro: "Inspect folder totals and file extensions. This page cannot delete or move files.",
  choosingFolder: "Choosing folder…",
  chooseFolder: "Choose folder",
  depthLabel: "Displayed folder depth",
  analyze: "Analyze folder",
  depthHelp:
    "Depth 0–64 limits displayed folders, not subtree totals. Scans still have native traversal and retention limits.",
  depthInvalid: "Enter a whole number from 0 to 64.",
  selectedFolder: (path: string) => `Selected folder: ${path}`,
  chooseAgain: "Choose a folder again to start another scan. Folder authorizations are single-use.",
  rootTotals: "Scanned root totals",
  logicalSize: "Logical size",
  allocatedSize: "Allocated size",
  independentFiles: "Independent files",
  hardLinkEntries: "Additional hard-link entries",
  incompleteTotals: "Incomplete totals: inaccessible, changing, protected, or bounded-out entries may be omitted.",
  logicalNote:
    "Logical size is not reclaimable space. Hard links are counted once per subtree or extension; sibling totals may not add to the root.",
  viewLabel: "Analysis view",
  folders: "Folders",
  extensions: "Extensions",
  breadcrumbs: "Folder breadcrumbs",
  scannedRoot: "Scanned root",
  childFoldersOf: (name: string) => `Child folders of ${name}`,
  extensionsHeading: "Extensions across the scanned root",
  currentSubtree: (logical: string, allocated: string) => `Current subtree: ${logical} logical · ${allocated} allocated`,
  noChildFolders: "No child folders within the displayed depth. Totals can still include deeper files.",
  noExtensions: "No file extensions found.",
  childFoldersLabel: "Child folders",
  fileExtensionsLabel: "File extensions",
  folderRow: (logical: string, allocated: string, files: string) =>
    `${logical} logical · ${allocated} allocated · ${files} independent files`,
  partialSubtree: "Partial subtree totals",
  noExtension: "No extension",
  extensionRow: (count: number, files: string, logical: string, allocated: string) =>
    `${files} ${count === 1 ? "file" : "files"} · ${logical} logical · ${allocated} allocated`,
  partialExtension: "Partial extension totals",
  unknownAllocation: "Unknown allocation",
};

const es419 = {
  eyebrow: "Análisis de almacenamiento de solo lectura",
  heading: "Analizador de disco",
  intro: "Revisa los totales por carpeta y las extensiones de archivo. Esta página no puede eliminar ni mover archivos.",
  choosingFolder: "Eligiendo carpeta…",
  chooseFolder: "Elegir carpeta",
  depthLabel: "Profundidad de carpetas mostradas",
  analyze: "Analizar carpeta",
  depthHelp:
    "La profundidad 0–64 limita las carpetas que se muestran, no los totales de cada subárbol. Los análisis siguen teniendo límites nativos de recorrido y retención.",
  depthInvalid: "Escribe un número entero de 0 a 64.",
  selectedFolder: (path: string) => `Carpeta seleccionada: ${path}`,
  chooseAgain: "Vuelve a elegir una carpeta para iniciar otro análisis. Las autorizaciones de carpeta son de un solo uso.",
  rootTotals: "Totales de la raíz analizada",
  logicalSize: "Tamaño lógico",
  allocatedSize: "Tamaño asignado",
  independentFiles: "Archivos independientes",
  hardLinkEntries: "Entradas adicionales de vínculos físicos",
  incompleteTotals:
    "Totales incompletos: pueden faltar entradas inaccesibles, que están cambiando, protegidas o fuera de los límites.",
  logicalNote:
    "El tamaño lógico no es espacio recuperable. Los vínculos físicos se cuentan una vez por subárbol o extensión; la suma de carpetas hermanas puede no coincidir con la raíz.",
  viewLabel: "Vista del análisis",
  folders: "Carpetas",
  extensions: "Extensiones",
  breadcrumbs: "Ruta de carpetas",
  scannedRoot: "Raíz analizada",
  childFoldersOf: (name: string) => `Subcarpetas de ${name}`,
  extensionsHeading: "Extensiones en toda la raíz analizada",
  currentSubtree: (logical: string, allocated: string) =>
    `Subárbol actual: ${logical} lógico · ${allocated} asignado`,
  noChildFolders:
    "No hay subcarpetas dentro de la profundidad mostrada. Los totales pueden incluir archivos más profundos.",
  noExtensions: "No se encontraron extensiones de archivo.",
  childFoldersLabel: "Subcarpetas",
  fileExtensionsLabel: "Extensiones de archivo",
  folderRow: (logical: string, allocated: string, files: string) =>
    `${logical} lógico · ${allocated} asignado · ${files} archivos independientes`,
  partialSubtree: "Totales parciales del subárbol",
  noExtension: "Sin extensión",
  extensionRow: (count: number, files: string, logical: string, allocated: string) =>
    `${files} ${count === 1 ? "archivo" : "archivos"} · ${logical} lógico · ${allocated} asignado`,
  partialExtension: "Totales parciales de la extensión",
  unknownAllocation: "Asignación desconocida",
} satisfies Catalog<typeof en>;

export const diskAnalyzerStrings = { en, es419 };
export type DiskAnalyzerStrings = typeof en;
