import type { Catalog } from "../../shared/i18n/catalog";

const en = {
  describeHibernation: (enabled: boolean) => `Turn hibernation ${enabled ? "on" : "off"}`,
  describeScheme: (name: string) => `Switch power plan: ${name}`,
  eyebrow: "System",
  title: "Power and hibernation",
  intro: "See the hibernation state and the active power plan; changes only happen after you select them and confirm in Windows.",
  refresh: "Refresh",
  loading: "Reading power settings…",
  hibernationTitle: "Hibernation",
  hibernationState: (enabled: boolean) => `Hibernation is ${enabled ? "on" : "off"}.`,
  hiberfile: (size: string) => ` The hibernation file uses ${size}.`,
  offWarning: "Turning hibernation off also disables Fast Startup and removes the hibernation file.",
  toggleHibernation: (enabled: boolean) => `Turn hibernation ${enabled ? "on" : "off"}`,
  unsupported: "Hibernation is not supported on this device (the firmware or Windows does not offer it), so it cannot be changed here.",
  plansTitle: "Power plans",
  noPlans: "No power plans were reported.",
  active: " (active)",
};

const es419 = {
  describeHibernation: (enabled: boolean) => `${enabled ? "Activar" : "Desactivar"} la hibernación`,
  describeScheme: (name: string) => `Cambiar el plan de energía: ${name}`,
  eyebrow: "Sistema",
  title: "Energía e hibernación",
  intro: "Consulta el estado de la hibernación y el plan de energía activo; los cambios solo se hacen después de que los selecciones y confirmes en Windows.",
  refresh: "Actualizar",
  loading: "Leyendo la configuración de energía…",
  hibernationTitle: "Hibernación",
  hibernationState: (enabled: boolean) => `La hibernación está ${enabled ? "activada" : "desactivada"}.`,
  hiberfile: (size: string) => ` El archivo de hibernación ocupa ${size}.`,
  offWarning: "Desactivar la hibernación también deshabilita el Inicio rápido y elimina el archivo de hibernación.",
  toggleHibernation: (enabled: boolean) => `${enabled ? "Activar" : "Desactivar"} la hibernación`,
  unsupported: "La hibernación no es compatible con este dispositivo (el firmware o Windows no la ofrecen), así que no se puede cambiar aquí.",
  plansTitle: "Planes de energía",
  noPlans: "No se informó ningún plan de energía.",
  active: " (activo)",
} satisfies Catalog<typeof en>;

export const powerStrings = { en, es419 };
export type PowerStrings = typeof en;
