use std::sync::Arc;

use serde::Serialize;
use windows_platform::{
    optimizer::{OptimizerReport, optimizer_report},
    system_change::SystemChangeService,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OptimizerCommandError {
    code: &'static str,
    message: &'static str,
}

/// Individually reviewable proposals. There is no command that applies them
/// together; selected proposals go through the shared plan commands.
#[tauri::command]
pub(crate) async fn get_optimizer_proposals(
    service: tauri::State<'_, Arc<SystemChangeService>>,
) -> Result<OptimizerReport, OptimizerCommandError> {
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || optimizer_report(&service))
        .await
        .map_err(|_| OptimizerCommandError {
            code: "optimizerUnavailable",
            message: "Optimization proposals could not be prepared.",
        })
}
