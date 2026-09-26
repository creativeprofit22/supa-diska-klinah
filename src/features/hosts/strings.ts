import type { Catalog } from "../../shared/i18n/catalog";
import type { HostsFindingKind, HostsFindingSeverity, HostsLineKind } from "./types";

const en = {
  kind: {
    blank: "Blank", comment: "Comment", mapping: "Mapping", appDisabled: "Disabled by this app", invalid: "Not a valid entry",
  } satisfies Record<HostsLineKind, string>,
  finding: {
    redirect: "Redirect", securityBlocked: "Security site blocked", customMapping: "Custom mapping", invalid: "Invalid line",
  } satisfies Record<HostsFindingKind, string>,
  severity: { high: "High", low: "Low" } satisfies Record<HostsFindingSeverity, string>,
  describePart: (disable: boolean, line: number) => `${disable ? "disable" : "restore"} line ${line}`,
  describe: (parts: string) => `Edit hosts file: ${parts}`,
  loadError: "The hosts file could not be read.",
  eyebrow: "System",
  title: "Hosts file",
  intro: "Lists what the Windows hosts file redirects or blocks; entries are only commented out or restored, never deleted.",
  refresh: "Refresh",
  loading: "Reading the hosts file…",
  summary: (lines: string, bytes: string, shown: string | null) =>
    ` · ${lines} lines · ${bytes} bytes${shown !== null ? ` · only the first ${shown} lines are shown` : ""}`,
  backupNote:
    "Before editing, a backup of the hosts file is made. The file is only changed if it is still exactly as it was when you reviewed it; otherwise nothing is written and you need to refresh.",
  findingsTitle: "Findings",
  noFindings: "No findings.",
  findingLine: (line: number, detail: string) => ` · Line ${line}: ${detail}`,
  linesTitle: "Lines",
  maxLines: (max: number) => `At most ${max} lines can be changed at a time.`,
  toggleLine: (disable: boolean, line: number) => `${disable ? "Disable" : "Restore"} line ${line}`,
  line: (line: number) => `Line ${line}`,
};

const es419 = {
  kind: {
    blank: "En blanco", comment: "Comentario", mapping: "Asignación", appDisabled: "Desactivada por esta app", invalid: "No es una entrada válida",
  },
  finding: {
    redirect: "Redirección", securityBlocked: "Sitio de seguridad bloqueado", customMapping: "Asignación personalizada", invalid: "Línea no válida",
  },
  severity: { high: "Alto", low: "Bajo" },
  describePart: (disable: boolean, line: number) => `${disable ? "desactivar" : "restaurar"} la línea ${line}`,
  describe: (parts: string) => `Editar el archivo hosts: ${parts}`,
  loadError: "No se pudo leer el archivo hosts.",
  eyebrow: "Sistema",
  title: "Archivo hosts",
  intro: "Muestra lo que el archivo hosts de Windows redirige o bloquea; las entradas solo se comentan o se restauran, nunca se eliminan.",
  refresh: "Actualizar",
  loading: "Leyendo el archivo hosts…",
  summary: (lines: string, bytes: string, shown: string | null) =>
    ` · ${lines} líneas · ${bytes} bytes${shown !== null ? ` · solo se muestran las primeras ${shown} líneas` : ""}`,
  backupNote:
    "Antes de editar, se hace una copia de seguridad del archivo hosts. El archivo solo se cambia si sigue exactamente igual que cuando lo revisaste; si no, no se escribe nada y tienes que actualizar.",
  findingsTitle: "Hallazgos",
  noFindings: "No hay hallazgos.",
  findingLine: (line: number, detail: string) => ` · Línea ${line}: ${detail}`,
  linesTitle: "Líneas",
  maxLines: (max: number) => `Se pueden cambiar como máximo ${max} líneas a la vez.`,
  toggleLine: (disable: boolean, line: number) => `${disable ? "Desactivar" : "Restaurar"} la línea ${line}`,
  line: (line: number) => `Línea ${line}`,
} satisfies Catalog<typeof en>;

export const hostsStrings = { en, es419 };
export type HostsStrings = typeof en;
