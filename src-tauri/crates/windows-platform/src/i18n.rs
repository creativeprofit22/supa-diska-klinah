//! Native (Rust-side) user-visible strings: Windows confirmation dialogs and
//! the updater prompt. The web UI has its own catalogs under `src/**/strings.ts`.
//!
//! The active locale follows the app's language setting; `System` follows the
//! Windows display language via `GetUserPreferredUILanguages`. Every Spanish
//! variant resolves to Latin American Spanish (`es-419`); everything else is English.

use cleanup_core::system_change::{PlannedChange, Privilege, RestartRequirement, Reversibility};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum LanguagePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "es-419")]
    Es419,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    En,
    Es419,
}

/// Maps one BCP 47 tag to a shipped locale.
pub fn match_locale(tag: &str) -> Option<Locale> {
    let primary = tag
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match primary.as_str() {
        "es" => Some(Locale::Es419),
        "en" => Some(Locale::En),
        _ => None,
    }
}

pub fn resolve_locale<'a>(
    preference: LanguagePreference,
    languages: impl IntoIterator<Item = &'a str>,
) -> Locale {
    match preference {
        LanguagePreference::En => Locale::En,
        LanguagePreference::Es419 => Locale::Es419,
        LanguagePreference::System => languages
            .into_iter()
            .find_map(match_locale)
            .unwrap_or(Locale::En),
    }
}

// The saved preference is process-wide state: native dialogs are raised deep
// inside services that have no settings handle, and threading one through every
// confirmation path would couple unrelated services to app settings. It holds
// only a display preference and is written by `AppSettingsService` alone.
static PREFERENCE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn set_preference(preference: LanguagePreference) {
    let value = match preference {
        LanguagePreference::System => 0,
        LanguagePreference::En => 1,
        LanguagePreference::Es419 => 2,
    };
    PREFERENCE.store(value, Ordering::Relaxed);
}

fn preference() -> LanguagePreference {
    match PREFERENCE.load(Ordering::Relaxed) {
        1 => LanguagePreference::En,
        2 => LanguagePreference::Es419,
        _ => LanguagePreference::System,
    }
}

/// The locale for native dialogs raised now.
pub fn current_locale() -> Locale {
    let languages = windows_ui_languages();
    resolve_locale(preference(), languages.iter().map(String::as_str))
}

#[cfg(windows)]
fn windows_ui_languages() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0_u32;
    let mut length = 0_u32;
    // SAFETY: a null buffer asks only for the required length.
    let sized = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut length,
        )
    };
    if sized == 0 || length == 0 || length > 4_096 {
        return Vec::new();
    }
    let mut buffer = vec![0_u16; length as usize];
    // SAFETY: the buffer holds exactly `length` UTF-16 units as requested.
    let ok = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    // A double-NUL-terminated multi-string.
    buffer
        .split(|&unit| unit == 0)
        .filter(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(not(windows))]
fn windows_ui_languages() -> Vec<String> {
    Vec::new()
}

/// Every native string. Each entry has an English and a Spanish (es-419) form.
/// Entries that take arguments are `fn` pointers so each text keeps a single
/// definition per language, with both languages side by side below.
#[derive(Clone, Copy, Debug)]
pub struct NativeStrings {
    // System changes (`system_change`).
    pub confirm_system_changes_title: &'static str,
    /// `count` → the opening question, including the blank line after it.
    pub system_changes_intro: fn(usize) -> String,
    /// One change line (the caller Debug-quotes it).
    pub system_change_line: fn(&PlannedChange) -> String,
    pub system_changes_admin_note: &'static str,
    pub system_changes_irreversible_note: &'static str,
    // Vendor uninstall (`storage::vendor_jobs`).
    pub confirm_vendor_uninstall_title: &'static str,
    /// Program, job ID, MSI?, executable, arguments. Names are Debug-quoted.
    pub vendor_uninstall_body: fn(&str, &str, bool, &Path, &[String]) -> String,
    // Permanent cleanup (`cleanup::execution`).
    pub confirm_permanent_cleanup_title: &'static str,
    /// Item count, logical bytes, plan ID.
    pub permanent_cleanup_intro: fn(usize, u64, &str) -> String,
    pub permanent_cleanup_omitted: &'static str,
    // Build artifacts (`cleanup::build_artifacts`).
    pub approve_build_profile_title: &'static str,
    /// Executable → text up to the `Arguments:` heading.
    pub build_profile_intro: fn(&str) -> String,
    pub build_profile_no_arguments: &'static str,
    pub build_profile_working_directory: &'static str,
    pub build_profile_artifact_paths: &'static str,
    // Folder pickers.
    pub pick_storage_folder_title: &'static str,
    pub pick_scan_folder_title: &'static str,
    pub pick_rule_pack_folder_title: &'static str,
    // Protection confirmations.
    pub quarantine_file_title: &'static str,
    pub quarantine_file_body: fn(&str) -> String,
    pub restore_file_title: &'static str,
    pub restore_file_body: fn(&str) -> String,
    pub delete_quarantined_title: &'static str,
    pub delete_quarantined_body: fn(&str) -> String,
    pub damaged_quarantine_entry: &'static str,
    pub restore_previous_rule_pack_title: &'static str,
    pub restore_previous_rule_pack_body: &'static str,
    // Updater.
    pub update_install_title: &'static str,
    /// `(from, to)` versions → the install confirmation body.
    pub update_install_body: fn(&str, &str) -> String,
}

const EN: NativeStrings = NativeStrings {
    confirm_system_changes_title: "Confirm system changes",
    system_changes_intro: |count| format!("Apply {count} system change(s)?\n\n"),
    system_change_line: PlannedChange::summary_line,
    system_changes_admin_note: "\nWindows will ask for administrator approval once for the marked changes.",
    system_changes_irreversible_note: "\nSome changes CANNOT be undone.",
    confirm_vendor_uninstall_title: "Confirm vendor uninstall",
    vendor_uninstall_body: |program, job_id, msi, executable, arguments| {
        format!(
            "Run vendor uninstall? This cannot be undone.\nProgram: {program:?}\nJob ID: {job_id}\nFamily: {}\nExecutable: {executable:?}\nArguments: {arguments:?}\n\nVendor UI/UAC may follow. Launcher exit is not proof of removal. Cancelling waiting does not terminate the vendor installer.",
            if msi {
                "Windows Installer"
            } else {
                "Vendor executable"
            },
        )
    },
    confirm_permanent_cleanup_title: "Confirm permanent cleanup",
    permanent_cleanup_intro: |count, bytes, plan_id| {
        format!(
            "Permanently delete {count} selected items ({bytes} logical bytes)?\nNo recovery is available.\nPlan: {plan_id}\n"
        )
    },
    permanent_cleanup_omitted: "\nAdditional selected items omitted.",
    approve_build_profile_title: "Approve build profile",
    build_profile_intro: |executable| {
        format!("Allow this repeatable build profile?\n\nExecutable:\n{executable}\n\nArguments:")
    },
    build_profile_no_arguments: "\n(none)",
    build_profile_working_directory: "\n\nWorking directory:\n",
    build_profile_artifact_paths: "\n\nArtifact paths:",
    pick_storage_folder_title: "Choose storage scan folder",
    pick_scan_folder_title: "Choose a folder to scan",
    pick_rule_pack_folder_title: "Choose the folder containing pack.json and pack.sig",
    quarantine_file_title: "Quarantine file",
    quarantine_file_body: |path| {
        format!(
            "Move this file into quarantine?\n\n{path}\n\nThe file is made non-executable and can be restored later from the Quarantine page."
        )
    },
    restore_file_title: "Restore file",
    restore_file_body: |target| {
        format!(
            "Restore this file from quarantine?\n\n{target}\n\nIt becomes executable again. If a file already exists there, nothing is overwritten and the restore is cancelled."
        )
    },
    delete_quarantined_title: "Delete permanently",
    delete_quarantined_body: |target| {
        format!("Permanently delete this quarantined file?\n\n{target}\n\nThis cannot be undone.")
    },
    damaged_quarantine_entry: "(damaged entry)",
    restore_previous_rule_pack_title: "Restore previous rule pack",
    restore_previous_rule_pack_body: "Switch back to the previously installed rule pack?\n\nThe newer pack will be removed. Only do this if the newer pack causes problems.",
    update_install_title: "Install update",
    update_install_body: |from, to| {
        format!(
            "Install Supa Diska Klinah {to}? (installed: {from})

The verified installer will start and this app will close. Windows will ask for administrator approval."
        )
    },
};

const ES_419: NativeStrings = NativeStrings {
    confirm_system_changes_title: "Confirmar cambios del sistema",
    system_changes_intro: |count| format!("¿Aplicar {count} cambio(s) del sistema?\n\n"),
    system_change_line: spanish_change_line,
    system_changes_admin_note: "\nWindows pedirá la aprobación de administrador una sola vez para los cambios marcados.",
    system_changes_irreversible_note: "\nAlgunos cambios NO se pueden deshacer.",
    confirm_vendor_uninstall_title: "Confirmar desinstalación del proveedor",
    vendor_uninstall_body: |program, job_id, msi, executable, arguments| {
        format!(
            "¿Ejecutar la desinstalación del proveedor? Esto no se puede deshacer.\nPrograma: {program:?}\nID de trabajo: {job_id}\nFamilia: {}\nEjecutable: {executable:?}\nArgumentos: {arguments:?}\n\nPuede aparecer la interfaz del proveedor o el Control de cuentas de usuario. Que el iniciador termine no prueba que se haya desinstalado. Cancelar la espera no detiene el instalador del proveedor.",
            if msi {
                "Windows Installer"
            } else {
                "Ejecutable del proveedor"
            },
        )
    },
    confirm_permanent_cleanup_title: "Confirmar limpieza permanente",
    permanent_cleanup_intro: |count, bytes, plan_id| {
        format!(
            "¿Eliminar permanentemente {count} elementos seleccionados ({bytes} bytes lógicos)?\nNo hay forma de recuperarlos.\nPlan: {plan_id}\n"
        )
    },
    permanent_cleanup_omitted: "\nSe omitieron elementos seleccionados adicionales.",
    approve_build_profile_title: "Aprobar perfil de compilación",
    build_profile_intro: |executable| {
        format!(
            "¿Permitir este perfil de compilación repetible?\n\nEjecutable:\n{executable}\n\nArgumentos:"
        )
    },
    build_profile_no_arguments: "\n(ninguno)",
    build_profile_working_directory: "\n\nDirectorio de trabajo:\n",
    build_profile_artifact_paths: "\n\nRutas de artefactos:",
    pick_storage_folder_title: "Elige la carpeta para analizar el almacenamiento",
    pick_scan_folder_title: "Elige una carpeta para analizar",
    pick_rule_pack_folder_title: "Elige la carpeta que contiene pack.json y pack.sig",
    quarantine_file_title: "Poner archivo en cuarentena",
    quarantine_file_body: |path| {
        format!(
            "¿Mover este archivo a cuarentena?\n\n{path}\n\nEl archivo deja de ser ejecutable y puedes restaurarlo más tarde desde la página Cuarentena."
        )
    },
    restore_file_title: "Restaurar archivo",
    restore_file_body: |target| {
        format!(
            "¿Restaurar este archivo desde la cuarentena?\n\n{target}\n\nVolverá a ser ejecutable. Si ya existe un archivo en esa ubicación, no se sobrescribe nada y la restauración se cancela."
        )
    },
    delete_quarantined_title: "Eliminar permanentemente",
    delete_quarantined_body: |target| {
        format!(
            "¿Eliminar permanentemente este archivo en cuarentena?\n\n{target}\n\nEsto no se puede deshacer."
        )
    },
    damaged_quarantine_entry: "(entrada dañada)",
    restore_previous_rule_pack_title: "Restaurar el paquete de reglas anterior",
    restore_previous_rule_pack_body: "¿Volver al paquete de reglas instalado anteriormente?\n\nSe quitará el paquete más reciente. Hazlo solo si el paquete más reciente causa problemas.",
    update_install_title: "Instalar actualización",
    update_install_body: |from, to| {
        format!(
            "¿Instalar Supa Diska Klinah {to}? (instalada: {from})

Se iniciará el instalador verificado y esta aplicación se cerrará. Windows pedirá la aprobación de administrador."
        )
    },
};

/// Spanish counterpart of `PlannedChange::summary_line` (same shape). The
/// component, effect and irreversibility reason come from the change catalog.
fn spanish_change_line(change: &PlannedChange) -> String {
    let reversibility = match &change.reversibility {
        Reversibility::Reversible => "se puede deshacer".to_owned(),
        Reversibility::ReversibleWithBackup => {
            "se puede deshacer desde una copia de seguridad".to_owned()
        }
        Reversibility::Irreversible { reason } => format!("NO se puede deshacer: {reason}"),
    };
    let restart = match change.impact.restart {
        RestartRequirement::None => "",
        RestartRequirement::SignOut => "; requiere cerrar sesión",
        RestartRequirement::Reboot => "; requiere reiniciar",
    };
    let admin = if change.privilege == Privilege::Helper {
        "; requiere administrador"
    } else {
        ""
    };
    format!(
        "{}: {} ({reversibility}{restart}{admin})",
        change.impact.component, change.impact.effect
    )
}

pub fn strings(locale: Locale) -> &'static NativeStrings {
    match locale {
        Locale::En => &EN,
        Locale::Es419 => &ES_419,
    }
}

/// Strings for the locale in effect right now.
pub fn native() -> &'static NativeStrings {
    strings(current_locale())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_variants_resolve_to_latin_american_spanish() {
        for tag in ["es", "es-MX", "es-AR", "es-CO", "es-ES", "es-419", "ES_mx"] {
            assert_eq!(match_locale(tag), Some(Locale::Es419), "{tag}");
        }
        assert_eq!(match_locale("en-GB"), Some(Locale::En));
        assert_eq!(match_locale("est"), None);
        assert_eq!(match_locale("fr-FR"), None);
    }

    #[test]
    fn preference_overrides_windows_and_unknown_falls_back_to_english() {
        assert_eq!(
            resolve_locale(LanguagePreference::System, ["fr-FR", "es-MX"]),
            Locale::Es419
        );
        assert_eq!(
            resolve_locale(LanguagePreference::System, ["fr-FR", "de-DE"]),
            Locale::En
        );
        assert_eq!(resolve_locale(LanguagePreference::System, []), Locale::En);
        assert_eq!(
            resolve_locale(LanguagePreference::En, ["es-MX"]),
            Locale::En
        );
        assert_eq!(
            resolve_locale(LanguagePreference::Es419, ["en-US"]),
            Locale::Es419
        );
    }

    #[test]
    fn preference_serializes_as_bcp47_tags() {
        assert_eq!(
            serde_json::to_string(&LanguagePreference::Es419).unwrap(),
            "\"es-419\""
        );
        assert!(serde_json::from_str::<LanguagePreference>("\"es-MX\"").is_err());
    }

    /// Renders every entry with sample arguments. The exhaustive destructuring
    /// makes a new field fail to compile until it is covered here.
    fn render(strings: &NativeStrings) -> Vec<String> {
        let NativeStrings {
            confirm_system_changes_title,
            system_changes_intro,
            // Needs a planned change; covered by `system_change::tests`.
            system_change_line: _,
            system_changes_admin_note,
            system_changes_irreversible_note,
            confirm_vendor_uninstall_title,
            vendor_uninstall_body,
            confirm_permanent_cleanup_title,
            permanent_cleanup_intro,
            permanent_cleanup_omitted,
            approve_build_profile_title,
            build_profile_intro,
            build_profile_no_arguments,
            build_profile_working_directory,
            build_profile_artifact_paths,
            pick_storage_folder_title,
            pick_scan_folder_title,
            pick_rule_pack_folder_title,
            quarantine_file_title,
            quarantine_file_body,
            restore_file_title,
            restore_file_body,
            delete_quarantined_title,
            delete_quarantined_body,
            damaged_quarantine_entry,
            restore_previous_rule_pack_title,
            restore_previous_rule_pack_body,
            update_install_title,
            update_install_body,
        } = *strings;
        let arguments = ["/x".to_owned()];
        vec![
            confirm_system_changes_title.to_owned(),
            system_changes_intro(2),
            system_changes_admin_note.to_owned(),
            system_changes_irreversible_note.to_owned(),
            confirm_vendor_uninstall_title.to_owned(),
            vendor_uninstall_body("P", "id", false, Path::new("u.exe"), &arguments),
            vendor_uninstall_body("P", "id", true, Path::new("msiexec.exe"), &arguments),
            confirm_permanent_cleanup_title.to_owned(),
            permanent_cleanup_intro(2, 10, "plan"),
            permanent_cleanup_omitted.to_owned(),
            approve_build_profile_title.to_owned(),
            build_profile_intro("cargo.exe"),
            build_profile_no_arguments.to_owned(),
            build_profile_working_directory.to_owned(),
            build_profile_artifact_paths.to_owned(),
            pick_storage_folder_title.to_owned(),
            pick_scan_folder_title.to_owned(),
            pick_rule_pack_folder_title.to_owned(),
            quarantine_file_title.to_owned(),
            quarantine_file_body("C:\\f.exe"),
            restore_file_title.to_owned(),
            restore_file_body("C:\\f.exe"),
            delete_quarantined_title.to_owned(),
            delete_quarantined_body("C:\\f.exe"),
            damaged_quarantine_entry.to_owned(),
            restore_previous_rule_pack_title.to_owned(),
            restore_previous_rule_pack_body.to_owned(),
            update_install_title.to_owned(),
            update_install_body("0.1.0", "0.2.0"),
        ]
    }

    #[test]
    fn every_native_string_is_translated() {
        let english = render(strings(Locale::En));
        let spanish = render(strings(Locale::Es419));
        assert_eq!(english.len(), spanish.len());
        for (english, spanish) in english.iter().zip(&spanish) {
            assert!(!english.trim().is_empty() && !spanish.trim().is_empty());
            assert_ne!(english, spanish);
        }
    }

    #[test]
    fn spanish_keeps_warnings_unsoftened() {
        let es = strings(Locale::Es419);
        assert_eq!(
            es.system_changes_irreversible_note,
            "\nAlgunos cambios NO se pueden deshacer."
        );
        assert!(
            (es.vendor_uninstall_body)("P", "id", false, Path::new("u.exe"), &[])
                .contains("Esto no se puede deshacer.")
        );
        assert!((es.delete_quarantined_body)("x").ends_with("Esto no se puede deshacer."));
    }
}
