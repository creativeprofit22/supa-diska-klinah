import type { Catalog } from "../../shared/i18n/catalog";
import type { ServiceStartType } from "../../shared/system-change/types";
import type { ProposalGroup } from "./types";

const en = {
  group: { services: "Services", privacy: "Privacy", performance: "Performance", power: "Power" } satisfies Record<ProposalGroup, string>,
  startType: { automatic: "automatic", manual: "manual", disabled: "disabled" } satisfies Record<ServiceStartType, string>,
  describe: {
    service: (id: string, startType: string) => `Set service ${id} to ${startType}`,
    setting: (id: string) => `Change setting: ${id}`,
    task: (enabled: boolean, id: string) => `${enabled ? "Enable" : "Disable"} task: ${id}`,
    powerPlan: (scheme: string) => `Switch power plan: ${scheme}`,
    hibernation: (enabled: boolean) => `${enabled ? "Enable" : "Disable"} hibernation`,
    other: (kind: string) => `System change: ${kind}`,
  },
  eyebrow: "System",
  title: "Quick optimization",
  intro: "Every change is listed separately. Suggested ones are pre-selected because they are low-risk and can be undone; nothing happens until you review the selection and confirm in Windows.",
  refresh: "Refresh",
  loading: "Checking which optimizations apply to this device…",
  summary: (applied: string, unavailable: string) => `${applied} already applied, ${unavailable} unavailable on this device`,
  none: "No optimizations to propose.",
  selectSuggested: "Select suggested",
  clearSelection: "Clear selection",
  maxChanges: (max: number) => `At most ${max} changes can be reviewed at a time.`,
  suggested: " · Suggested",
  reviewLabel: "Review selected optimizations",
};

const es419 = {
  group: { services: "Servicios", privacy: "Privacidad", performance: "Rendimiento", power: "Energía" },
  startType: { automatic: "automático", manual: "manual", disabled: "deshabilitado" },
  describe: {
    service: (id: string, startType: string) => `Establecer el servicio ${id} en ${startType}`,
    setting: (id: string) => `Cambiar la configuración: ${id}`,
    task: (enabled: boolean, id: string) => `${enabled ? "Habilitar" : "Deshabilitar"} la tarea: ${id}`,
    powerPlan: (scheme: string) => `Cambiar el plan de energía: ${scheme}`,
    hibernation: (enabled: boolean) => `${enabled ? "Habilitar" : "Deshabilitar"} la hibernación`,
    other: (kind: string) => `Cambio del sistema: ${kind}`,
  },
  eyebrow: "Sistema",
  title: "Optimización rápida",
  intro: "Cada cambio aparece por separado. Los sugeridos vienen preseleccionados porque son de bajo riesgo y se pueden deshacer; no pasa nada hasta que revises la selección y confirmes en Windows.",
  refresh: "Actualizar",
  loading: "Comprobando qué optimizaciones se aplican a este dispositivo…",
  summary: (applied: string, unavailable: string) => `${applied} ya aplicadas, ${unavailable} no disponibles en este dispositivo`,
  none: "No hay optimizaciones para proponer.",
  selectSuggested: "Seleccionar sugeridas",
  clearSelection: "Borrar selección",
  maxChanges: (max: number) => `Se pueden revisar como máximo ${max} cambios a la vez.`,
  suggested: " · Sugerida",
  reviewLabel: "Revisar las optimizaciones seleccionadas",
} satisfies Catalog<typeof en>;

export const optimizerStrings = { en, es419 };
export type OptimizerStrings = typeof en;
