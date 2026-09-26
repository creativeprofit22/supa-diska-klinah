import type { Catalog } from "../../shared/i18n/catalog";
import type { ServiceStartState } from "../../shared/system-change/types";
import type { ServiceCategory } from "./types";

const en = {
  start: {
    boot: "Boot", system: "System", automatic: "Automatic", manual: "Manual", disabled: "Disabled",
  } satisfies Record<ServiceStartState, string>,
  category: {
    telemetry: "Telemetry", gaming: "Gaming", legacy: "Legacy", performance: "Performance", other: "Other",
  } satisfies Record<ServiceCategory, string>,
  lockedNotInstalled: "Not installed on this PC, so it cannot be changed.",
  lockedUnreadable: "Its current configuration could not be read, so it cannot be changed.",
  lockedBootSystem: "It is a boot or system driver start type, which this app never changes.",
  unknown: "Unknown",
  delayed: " (delayed)",
  describe: (startType: string, label: string) => `Set service start type to ${startType}: ${label}`,
  eyebrow: "System",
  title: "Windows services",
  intro: "Change how selected Windows services start; running services are not stopped or started.",
  refresh: "Refresh",
  loading: "Loading services…",
  none: "No services are listed.",
  recommended: (value: string) => ` · Recommended: ${value}`,
  current: (value: string) => ` · Current: ${value}`,
  notInstalled: "Not installed",
  running: " · Running",
  notRunning: " · Not running",
  startTypeFor: (label: string) => `Start type for ${label}`,
  keepCurrent: "Keep current",
};

const es419 = {
  start: {
    boot: "Arranque", system: "Sistema", automatic: "Automático", manual: "Manual", disabled: "Deshabilitado",
  },
  category: {
    telemetry: "Telemetría", gaming: "Juegos", legacy: "Heredado", performance: "Rendimiento", other: "Otro",
  },
  lockedNotInstalled: "No está instalado en esta PC, así que no se puede cambiar.",
  lockedUnreadable: "No se pudo leer su configuración actual, así que no se puede cambiar.",
  lockedBootSystem: "Tiene un tipo de inicio de controlador de arranque o del sistema, que esta app nunca cambia.",
  unknown: "Desconocido",
  delayed: " (retrasado)",
  describe: (startType: string, label: string) => `Establecer el tipo de inicio del servicio en ${startType}: ${label}`,
  eyebrow: "Sistema",
  title: "Servicios de Windows",
  intro: "Cambia cómo se inician los servicios de Windows seleccionados; los servicios en ejecución no se detienen ni se inician.",
  refresh: "Actualizar",
  loading: "Cargando servicios…",
  none: "No hay servicios en la lista.",
  recommended: (value: string) => ` · Recomendado: ${value}`,
  current: (value: string) => ` · Actual: ${value}`,
  notInstalled: "No instalado",
  running: " · En ejecución",
  notRunning: " · Detenido",
  startTypeFor: (label: string) => `Tipo de inicio para ${label}`,
  keepCurrent: "Mantener el actual",
} satisfies Catalog<typeof en>;

export const servicesStrings = { en, es419 };
export type ServicesStrings = typeof en;
