use serde::Serialize;
use windows_platform::cleanup::{CleanupPreview, CleanupPreviewError};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupCommandError {
    code: &'static str,
    message: &'static str,
}

impl CleanupCommandError {
    fn task_unavailable() -> Self {
        Self {
            code: "scanUnavailable",
            message: "Cleanup preview could not be started.",
        }
    }
}

impl From<CleanupPreviewError> for CleanupCommandError {
    fn from(error: CleanupPreviewError) -> Self {
        let (code, message) = match error {
            CleanupPreviewError::TemporaryRootUnavailable => (
                "temporaryRootUnavailable",
                "The Windows temporary folder is unavailable.",
            ),
            CleanupPreviewError::ProtectionUnavailable => (
                "protectionUnavailable",
                "Required protected folders are unavailable.",
            ),
            CleanupPreviewError::ProtectionInvalid => (
                "protectionInvalid",
                "Required protected folders could not be validated.",
            ),
            CleanupPreviewError::CatalogInvalid => (
                "catalogInvalid",
                "Cleanup preview configuration is unavailable.",
            ),
            CleanupPreviewError::ScanFailed => {
                ("scanFailed", "Cleanup preview could not be completed.")
            }
        };
        Self { code, message }
    }
}

#[tauri::command]
pub(crate) async fn preview_cleanup() -> Result<CleanupPreview, CleanupCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::cleanup::preview_temporary_caches)
        .await
        .map_err(|_| CleanupCommandError::task_unavailable())?
        .map_err(CleanupCommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_failures_map_to_stable_non_sensitive_errors() {
        for (error, expected_code, expected_message) in [
            (
                CleanupPreviewError::TemporaryRootUnavailable,
                "temporaryRootUnavailable",
                "The Windows temporary folder is unavailable.",
            ),
            (
                CleanupPreviewError::ProtectionUnavailable,
                "protectionUnavailable",
                "Required protected folders are unavailable.",
            ),
            (
                CleanupPreviewError::ProtectionInvalid,
                "protectionInvalid",
                "Required protected folders could not be validated.",
            ),
            (
                CleanupPreviewError::CatalogInvalid,
                "catalogInvalid",
                "Cleanup preview configuration is unavailable.",
            ),
            (
                CleanupPreviewError::ScanFailed,
                "scanFailed",
                "Cleanup preview could not be completed.",
            ),
        ] {
            let command_error = CleanupCommandError::from(error);
            assert_eq!(command_error.code, expected_code);
            assert_eq!(command_error.message, expected_message);
            assert!(!command_error.message.contains('\\'));
            assert!(!command_error.message.contains("C:"));
        }
    }
}
