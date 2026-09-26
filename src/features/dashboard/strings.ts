import type { Catalog } from "../../shared/i18n/catalog";

const en = {
  documentTitle: "Dashboard | Supa Diska Klinah",
  kicker: "Foundation",
  heading: "System readiness",
  intro: "Confirm the native Windows adapter before cleanup features arrive.",
  checking: "Checking the native adapter…",
  unreachable: "Could not reach the native adapter",
  tryAgain: "Try again",
  adapterStatus: "Adapter status",
  ready: "Ready",
  unavailable: "Unavailable",
  platform: "Platform",
  architecture: "Architecture",
  nativeAdapter: "Native adapter",
  connected: "Connected",
  notConnected: "Not connected",
  restorePointHeading: "System restore point",
  restorePointBody: "Save Windows system settings before future cleanup actions. This is not a file backup.",
  restorePointLink: "Create or view restore points",
};

const es419 = {
  documentTitle: "Panel | Supa Diska Klinah",
  kicker: "Base",
  heading: "Preparación del sistema",
  intro: "Confirma el adaptador nativo de Windows antes de que lleguen las funciones de limpieza.",
  checking: "Revisando el adaptador nativo…",
  unreachable: "No se pudo conectar con el adaptador nativo",
  tryAgain: "Intentar de nuevo",
  adapterStatus: "Estado del adaptador",
  ready: "Listo",
  unavailable: "No disponible",
  platform: "Plataforma",
  architecture: "Arquitectura",
  nativeAdapter: "Adaptador nativo",
  connected: "Conectado",
  notConnected: "No conectado",
  restorePointHeading: "Punto de restauración del sistema",
  restorePointBody:
    "Guarda la configuración del sistema de Windows antes de futuras acciones de limpieza. No es una copia de seguridad de archivos.",
  restorePointLink: "Crear o ver puntos de restauración",
} satisfies Catalog<typeof en>;

export const dashboardStrings = { en, es419 };
export type DashboardStrings = typeof en;
