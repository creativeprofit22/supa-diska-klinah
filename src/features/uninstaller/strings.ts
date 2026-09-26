import type { Catalog } from "../../shared/i18n/catalog";
import type { VendorJobState } from "./api";

const en = {
  eyebrow: "Separate vendor operations",
  heading: "Installed programs",
  intro:
    "Read-only registry inventory. Select one program explicitly to review a separate vendor uninstall job. No programs are selected automatically.",
  programName: "Program name",
  sort: "Sort",
  sortName: "Name",
  sortSize: "Estimated size, largest first",
  nameInvalid: "Name must be at most 128 UTF-16 units, without control characters.",
  refreshInventory: "Refresh inventory",
  idle: "Refresh inventory to list installed programs.",
  sizesNote:
    "Sizes are vendor registry estimates, not measured disk use or reclaimed space. Install dates are vendor-reported; last use is unknown when unavailable. Refresh inventory to observe changes; it does not prove removal.",
  partialInventory: "Partial inventory: bounded enumeration or unavailable registry entries may omit programs.",
  resultsLabel: "Installed program results",
  unknown: "Unknown",
  publisherVersion: (publisher: string, version: string) => `Publisher: ${publisher} · Version: ${version}`,
  programDetails: (size: string, installDate: string, lastUse: string) =>
    `Estimated size: ${size} · Vendor-reported install date: ${installDate} · Last use: ${lastUse}`,
  leftovers: "Leftovers: unknown ownership; informational only, not selectable.",
  reviewUninstall: (name: string) => `Review vendor uninstall for ${name}`,
  noMatches: "No matching programs on this page.",
  waiting: "Waiting for the vendor request or Windows confirmation…",
  reviewLabel: "Immutable vendor job review",
  jobId: (id: string) => `Job ID: ${id}`,
  evidenceUnavailable: "Current vendor evidence unavailable.",
  exitCode: (code: number) => `Launcher exit code: ${code}`,
  launchError: (code: number) => `Windows launch error code: ${code}`,
  persistenceError:
    "Journal persistence failed. The outcome may not survive restart; do not assume removal or retry automatically.",
  cannotUndo:
    "Vendor uninstall cannot be undone here. Windows confirmation will show the backend-resolved command; vendor UI or UAC may follow.",
  continueConfirmation: "Continue to Windows confirmation",
  cancelPrepared: "Cancel prepared job",
  stopWaiting: "Stop waiting (does not stop installer)",
  historyLabel: "Retained vendor job history",
  historyHeading: "Retained vendor jobs",
  historyPaging:
    "Up to 64 retained jobs per page, newest first. Navigation stops polling, not the vendor installer. Unknown outcomes cannot be erased or replayed.",
  historyCapacity:
    "Up to 1,000 outcomes are retained, subject to an 8 MiB storage limit. At capacity, new jobs stop until a future journal migration; retained history remains available for inspection.",
  refreshHistory: "Refresh retained history",
  olderHistory: "Older retained jobs",
  historyLoading: "Loading retained history…",
  historyNotLoaded: "Retained history has not been loaded.",
  historyEmpty: "No retained jobs reported.",
  historyEnd: "End of retained history.",
  outcome: {
    awaitingConfirmation: "Awaiting separate Windows confirmation. Nothing has launched.",
    queued: "Queued for vendor launch. Vendor UI or UAC may appear.",
    launching: "Waiting for the vendor launcher. Navigating away does not stop the installer.",
    completed: "Launcher exited successfully. This is not proof that the program was removed.",
    cancelledByVendorOrUAC: "Vendor or UAC declined/cancelled. Removal is not confirmed.",
    cancelledBeforeLaunch: "Cancelled before launch. No vendor installer was started by this job.",
    failed: "Vendor launch or operation failed. Removal is not confirmed.",
    rebootRequired: "Vendor reported a reboot is required. Removal is not yet confirmed.",
    outcomeUnknown:
      "Outcome unknown: timeout, interrupted waiting, or an unverified vendor exit. The installer may still be running; it was not terminated.",
  } satisfies Record<VendorJobState, string>,
  jobErrors: {
    evidenceRetired: "Program evidence expired or was retired. Refresh inventory and review again.",
    submittedUnknown:
      "The submitted job outcome is unknown. Refresh retained history to check again; do not retry the vendor operation.",
    evidenceUnavailable: "Program evidence is unavailable or expired. Refresh inventory and review again.",
    pollingLimit: "Polling limit reached. The vendor was not stopped. Refresh retained history for its outcome.",
    historyLoad:
      "Retained history could not be loaded. Refresh retained history to try again. History browsing does not start vendor jobs.",
  },
  errors: {
    historyCountCapacity:
      "Retained vendor history has reached its 1,000-outcome capacity. New jobs are stopped until a future journal migration. You can still inspect retained history.",
    historySizeCapacity:
      "Retained vendor history has reached its storage capacity (8 MiB safeguard). New jobs are stopped until a future journal migration. You can still inspect retained history.",
    busy: "Another operation or an unresolved vendor job is active.",
    expired: "Program evidence expired or changed. Refresh inventory and review again.",
    unsupported: "This vendor command is not supported. Use Windows installed-app settings.",
    unknown: "The vendor request could not be confirmed. Check retained history before trying again.",
  },
};

const es419 = {
  eyebrow: "Operaciones independientes del proveedor",
  heading: "Programas instalados",
  intro:
    "Inventario del registro de solo lectura. Selecciona explícitamente un programa para revisar un trabajo independiente de desinstalación del proveedor. No se selecciona ningún programa automáticamente.",
  programName: "Nombre del programa",
  sort: "Ordenar",
  sortName: "Nombre",
  sortSize: "Tamaño estimado, del más grande al más pequeño",
  nameInvalid: "El nombre debe tener como máximo 128 unidades UTF-16, sin caracteres de control.",
  refreshInventory: "Actualizar inventario",
  idle: "Actualiza el inventario para ver los programas instalados.",
  sizesNote:
    "Los tamaños son estimaciones del registro del proveedor, no uso de disco medido ni espacio recuperado. Las fechas de instalación las informa el proveedor; el último uso es desconocido cuando no está disponible. Actualiza el inventario para ver cambios; eso no demuestra que se haya desinstalado.",
  partialInventory:
    "Inventario parcial: la enumeración limitada o las entradas del registro no disponibles pueden omitir programas.",
  resultsLabel: "Resultados de programas instalados",
  unknown: "Desconocido",
  publisherVersion: (publisher: string, version: string) => `Editor: ${publisher} · Versión: ${version}`,
  programDetails: (size: string, installDate: string, lastUse: string) =>
    `Tamaño estimado: ${size} · Fecha de instalación informada por el proveedor: ${installDate} · Último uso: ${lastUse}`,
  leftovers: "Restos: propiedad desconocida; solo informativo, no se puede seleccionar.",
  reviewUninstall: (name: string) => `Revisar la desinstalación del proveedor para ${name}`,
  noMatches: "No hay programas coincidentes en esta página.",
  waiting: "Esperando la solicitud del proveedor o la confirmación de Windows…",
  reviewLabel: "Revisión inmutable del trabajo del proveedor",
  jobId: (id: string) => `ID del trabajo: ${id}`,
  evidenceUnavailable: "La evidencia actual del proveedor no está disponible.",
  exitCode: (code: number) => `Código de salida del iniciador: ${code}`,
  launchError: (code: number) => `Código de error de inicio de Windows: ${code}`,
  persistenceError:
    "No se pudo guardar el registro. Es posible que el resultado no se conserve al reiniciar; no des por hecho que se desinstaló ni lo reintentes automáticamente.",
  cannotUndo:
    "La desinstalación del proveedor no se puede deshacer aquí. La confirmación de Windows mostrará el comando resuelto por el sistema; después puede aparecer la interfaz del proveedor o UAC.",
  continueConfirmation: "Continuar a la confirmación de Windows",
  cancelPrepared: "Cancelar trabajo preparado",
  stopWaiting: "Dejar de esperar (no detiene el instalador)",
  historyLabel: "Historial de trabajos del proveedor conservados",
  historyHeading: "Trabajos del proveedor conservados",
  historyPaging:
    "Hasta 64 trabajos conservados por página, los más recientes primero. Salir de la página detiene la consulta, no el instalador del proveedor. Los resultados desconocidos no se pueden borrar ni repetir.",
  historyCapacity:
    "Se conservan hasta 1000 resultados, con un límite de almacenamiento de 8 MiB. Al alcanzar la capacidad, no se aceptan trabajos nuevos hasta una futura migración del registro; el historial conservado sigue disponible para consultarlo.",
  refreshHistory: "Actualizar historial conservado",
  olderHistory: "Trabajos conservados más antiguos",
  historyLoading: "Cargando el historial conservado…",
  historyNotLoaded: "El historial conservado no se ha cargado.",
  historyEmpty: "No se informaron trabajos conservados.",
  historyEnd: "Fin del historial conservado.",
  outcome: {
    awaitingConfirmation: "Esperando una confirmación aparte de Windows. No se ha iniciado nada.",
    queued: "En cola para el inicio del proveedor. Puede aparecer la interfaz del proveedor o UAC.",
    launching: "Esperando al iniciador del proveedor. Salir de la página no detiene el instalador.",
    completed: "El iniciador terminó correctamente. Esto no demuestra que el programa se haya desinstalado.",
    cancelledByVendorOrUAC: "El proveedor o UAC lo rechazó o canceló. No se confirma la desinstalación.",
    cancelledBeforeLaunch: "Cancelado antes de iniciar. Este trabajo no inició ningún instalador del proveedor.",
    failed: "Falló el inicio o la operación del proveedor. No se confirma la desinstalación.",
    rebootRequired: "El proveedor informó que se requiere reiniciar. Todavía no se confirma la desinstalación.",
    outcomeUnknown:
      "Resultado desconocido: tiempo agotado, espera interrumpida o una salida del proveedor no verificada. Es posible que el instalador siga en ejecución; no se terminó.",
  },
  jobErrors: {
    evidenceRetired:
      "La evidencia del programa venció o se retiró. Actualiza el inventario y vuelve a revisar.",
    submittedUnknown:
      "El resultado del trabajo enviado es desconocido. Actualiza el historial conservado para revisarlo de nuevo; no reintentes la operación del proveedor.",
    evidenceUnavailable:
      "La evidencia del programa no está disponible o venció. Actualiza el inventario y vuelve a revisar.",
    pollingLimit:
      "Se alcanzó el límite de consultas. El proveedor no se detuvo. Actualiza el historial conservado para ver su resultado.",
    historyLoad:
      "No se pudo cargar el historial conservado. Actualiza el historial conservado para intentarlo de nuevo. Consultar el historial no inicia trabajos del proveedor.",
  },
  errors: {
    historyCountCapacity:
      "El historial conservado del proveedor alcanzó su capacidad de 1000 resultados. No se aceptan trabajos nuevos hasta una futura migración del registro. Todavía puedes consultar el historial conservado.",
    historySizeCapacity:
      "El historial conservado del proveedor alcanzó su capacidad de almacenamiento (protección de 8 MiB). No se aceptan trabajos nuevos hasta una futura migración del registro. Todavía puedes consultar el historial conservado.",
    busy: "Hay otra operación o un trabajo del proveedor sin resolver en curso.",
    expired: "La evidencia del programa venció o cambió. Actualiza el inventario y vuelve a revisar.",
    unsupported: "Este comando del proveedor no es compatible. Usa la configuración de aplicaciones instaladas de Windows.",
    unknown: "No se pudo confirmar la solicitud del proveedor. Revisa el historial conservado antes de volver a intentarlo.",
  },
} satisfies Catalog<typeof en>;

export const uninstallerStrings = { en, es419 };
export type UninstallerStrings = typeof en;
