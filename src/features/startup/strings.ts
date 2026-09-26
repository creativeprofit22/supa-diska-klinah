import type { Catalog } from "../../shared/i18n/catalog";
import type { StartupSource } from "./types";

const en = {
  source: {
    run: "Run key", run32: "32-bit Run key", startupFolder: "Startup folder", runOnce: "RunOnce key", logonTask: "Scheduled task (at sign-in)",
  } satisfies Record<StartupSource, string>,
  readOnlyRunOnce: "Read-only: RunOnce entries run a single time and Windows removes them afterwards, so they cannot be turned on or off here.",
  readOnlyLogonTask: "Read-only: this is a scheduled task with a sign-in trigger. Manage it in Task Scheduler.",
  readOnly: "Read-only: this entry cannot be turned on or off here.",
  describe: (enabled: boolean, name: string) => `${enabled ? "Enable" : "Disable"} startup item: ${name}`,
  eyebrow: "System",
  title: "Startup apps",
  intro: "Choose which programs start when you sign in; turning one off only marks it disabled, like Task Manager does.",
  noDelete: "Deleting startup entries is not offered. Turning an item off keeps it listed here so you can turn it back on later.",
  refresh: "Refresh",
  loading: "Loading startup items…",
  none: "No startup items were found.",
  yourAccount: "Your account",
  allUsers: "All users",
  currently: (enabled: boolean) => ` · Currently ${enabled ? "enabled" : "disabled"}`,
  changeItem: (name: string, enable: boolean) => `Change ${name} to ${enable ? "enabled" : "disabled"}`,
  changeTo: (enable: boolean) => `Change to ${enable ? "enabled" : "disabled"}`,
};

const es419 = {
  source: {
    run: "Clave Run", run32: "Clave Run de 32 bits", startupFolder: "Carpeta Inicio", runOnce: "Clave RunOnce", logonTask: "Tarea programada (al iniciar sesión)",
  },
  readOnlyRunOnce: "Solo lectura: las entradas RunOnce se ejecutan una sola vez y Windows las quita después, así que no se pueden activar ni desactivar aquí.",
  readOnlyLogonTask: "Solo lectura: es una tarea programada que se activa al iniciar sesión. Adminístrala en el Programador de tareas.",
  readOnly: "Solo lectura: esta entrada no se puede activar ni desactivar aquí.",
  describe: (enabled: boolean, name: string) => `${enabled ? "Habilitar" : "Deshabilitar"} el elemento de inicio: ${name}`,
  eyebrow: "Sistema",
  title: "Aplicaciones de inicio",
  intro: "Elige qué programas se inician cuando inicias sesión; desactivar uno solo lo marca como deshabilitado, como lo hace el Administrador de tareas.",
  noDelete: "No se ofrece eliminar entradas de inicio. Si desactivas un elemento, sigue en esta lista para que puedas volver a activarlo más tarde.",
  refresh: "Actualizar",
  loading: "Cargando elementos de inicio…",
  none: "No se encontraron elementos de inicio.",
  yourAccount: "Tu cuenta",
  allUsers: "Todos los usuarios",
  currently: (enabled: boolean) => ` · Actualmente ${enabled ? "habilitado" : "deshabilitado"}`,
  changeItem: (name: string, enable: boolean) => `Cambiar ${name} a ${enable ? "habilitado" : "deshabilitado"}`,
  changeTo: (enable: boolean) => `Cambiar a ${enable ? "habilitado" : "deshabilitado"}`,
} satisfies Catalog<typeof en>;

export const startupStrings = { en, es419 };
export type StartupStrings = typeof en;
