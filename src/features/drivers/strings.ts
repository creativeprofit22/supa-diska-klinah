import type { Catalog } from "../../shared/i18n/catalog";
import type { DriverPackageStatus } from "./types";

const en = {
  restoreDescription: "Before removing driver packages",
  status: {
    inUse: "In use by a present device",
    current: "Not in use, but no newer package replaces it",
    superseded: "Superseded by a newer package",
  } satisfies Record<DriverPackageStatus, string>,
  notDeletableInUse: "Cannot be removed: a present device uses it.",
  notDeletableCurrent: "Cannot be removed: it is the newest package from this provider.",
  notDeletable: "Cannot be removed.",
  describeDelete: (name: string) => `Remove driver package: ${name}`,
  describeRestorePoint: (description: string) => `Create restore point: ${description}`,
  loadError: "Driver packages could not be listed.",
  eyebrow: "System",
  title: "Driver packages",
  intro: "Lists driver packages in the Windows driver store; only superseded packages not used by any device can be removed.",
  refresh: "Refresh",
  loading: "Loading driver packages…",
  irreversible: "Removing a driver package from the driver store cannot be undone by this app. Driver update installation is not offered.",
  none: "No driver packages reported.",
  statusLine: (status: string) => `Status: ${status}`,
  createRestorePoint: "Create a restore point first",
};

const es419 = {
  restoreDescription: "Antes de quitar paquetes de controladores",
  status: {
    inUse: "En uso por un dispositivo presente",
    current: "No está en uso, pero ningún paquete más reciente lo reemplaza",
    superseded: "Reemplazado por un paquete más reciente",
  },
  notDeletableInUse: "No se puede quitar: lo usa un dispositivo presente.",
  notDeletableCurrent: "No se puede quitar: es el paquete más reciente de este proveedor.",
  notDeletable: "No se puede quitar.",
  describeDelete: (name: string) => `Quitar paquete de controlador: ${name}`,
  describeRestorePoint: (description: string) => `Crear punto de restauración: ${description}`,
  loadError: "No se pudieron listar los paquetes de controladores.",
  eyebrow: "Sistema",
  title: "Paquetes de controladores",
  intro: "Muestra los paquetes de controladores del almacén de controladores de Windows; solo se pueden quitar los paquetes reemplazados que ningún dispositivo usa.",
  refresh: "Actualizar",
  loading: "Cargando paquetes de controladores…",
  irreversible: "Esta app no puede deshacer la eliminación de un paquete del almacén de controladores. No se ofrece instalar actualizaciones de controladores.",
  none: "No se informó ningún paquete de controlador.",
  statusLine: (status: string) => `Estado: ${status}`,
  createRestorePoint: "Crear primero un punto de restauración",
} satisfies Catalog<typeof en>;

export const driversStrings = { en, es419 };
export type DriversStrings = typeof en;
