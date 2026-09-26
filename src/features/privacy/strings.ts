import type { Catalog } from "../../shared/i18n/catalog";
import type { SettingCategory } from "./types";

const en = {
  category: { privacy: "Privacy", performance: "Performance" } satisfies Record<SettingCategory, string>,
  restoreDefaultFor: (label: string) => `Restore Windows default for ${label}`,
  applyRecommendedFor: (label: string) => `Apply recommended for ${label}`,
  windowsDefault: "Windows default",
  describeRestore: (label: string) => `Restore Windows default: ${label}`,
  describeApply: (label: string) => `Apply recommended setting: ${label}`,
  describeTask: (enabled: boolean, label: string) => `${enabled ? "Enable" : "Disable"} scheduled task: ${label}`,
  eyebrow: "System",
  title: "Privacy",
  intro: "Review Windows privacy and telemetry settings; nothing changes until you select an item and confirm it in Windows.",
  refresh: "Refresh",
  loading: "Reading privacy settings…",
  yourAccount: "Your account",
  allUsers: "All users (needs administrator)",
  current: (value: string) => ` · Current: ${value}`,
  recommended: (value: string) => ` · Recommended: ${value}`,
  recommendedApplied: " · Recommended value applied",
  unavailable: (reason: string) => `Unavailable: ${reason}`,
  tasksTitle: "Scheduled tasks",
  noTasks: "No telemetry tasks are listed.",
  taskNotPresent: "Not present on this device",
  taskState: (enabled: boolean, recommended: boolean) =>
    `${enabled ? "Enabled" : "Disabled"} · Recommended: ${recommended ? "enabled" : "disabled"}`,
  relatedServices: (services: string) => `Related telemetry services (${services}) are managed on the Services page.`,
};

const es419 = {
  category: { privacy: "Privacidad", performance: "Rendimiento" },
  restoreDefaultFor: (label: string) => `Restaurar el valor predeterminado de Windows para ${label}`,
  applyRecommendedFor: (label: string) => `Aplicar lo recomendado para ${label}`,
  windowsDefault: "Predeterminado de Windows",
  describeRestore: (label: string) => `Restaurar el valor predeterminado de Windows: ${label}`,
  describeApply: (label: string) => `Aplicar la configuración recomendada: ${label}`,
  describeTask: (enabled: boolean, label: string) => `${enabled ? "Habilitar" : "Deshabilitar"} la tarea programada: ${label}`,
  eyebrow: "Sistema",
  title: "Privacidad",
  intro: "Revisa la configuración de privacidad y telemetría de Windows; nada cambia hasta que selecciones un elemento y lo confirmes en Windows.",
  refresh: "Actualizar",
  loading: "Leyendo la configuración de privacidad…",
  yourAccount: "Tu cuenta",
  allUsers: "Todos los usuarios (requiere administrador)",
  current: (value: string) => ` · Actual: ${value}`,
  recommended: (value: string) => ` · Recomendado: ${value}`,
  recommendedApplied: " · Valor recomendado aplicado",
  unavailable: (reason: string) => `No disponible: ${reason}`,
  tasksTitle: "Tareas programadas",
  noTasks: "No hay tareas de telemetría en la lista.",
  taskNotPresent: "No existe en este dispositivo",
  taskState: (enabled: boolean, recommended: boolean) =>
    `${enabled ? "Habilitada" : "Deshabilitada"} · Recomendado: ${recommended ? "habilitada" : "deshabilitada"}`,
  relatedServices: (services: string) => `Los servicios de telemetría relacionados (${services}) se administran en la página Servicios.`,
} satisfies Catalog<typeof en>;

export const privacyStrings = { en, es419 };
export type PrivacyStrings = typeof en;
