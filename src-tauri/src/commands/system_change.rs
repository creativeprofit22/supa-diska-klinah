// Shared preview / plan / native-confirm / execute / journal / rollback
// commands for every system-management module. The webview sends typed
// `SystemChange` values and opaque plan or journal identifiers only.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use windows_platform::system_change::{
    JournalView, PlanTicket, SystemChangeError, SystemChangeService,
    contract::{ExecutionReport, MAX_PLAN_CHANGES, PlannedChange, SystemChange},
    decode_request,
};

const MAX_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SystemChangeCommandError {
    code: SystemChangeError,
    message: String,
}

impl From<SystemChangeError> for SystemChangeCommandError {
    fn from(code: SystemChangeError) -> Self {
        Self {
            code,
            message: code.to_string(),
        }
    }
}

fn invalid() -> SystemChangeCommandError {
    SystemChangeError::InvalidChange.into()
}

fn unavailable() -> SystemChangeCommandError {
    SystemChangeError::JournalUnavailable.into()
}

fn decode<T: serde::de::DeserializeOwned>(
    request: &tauri::ipc::Request<'_>,
) -> Result<T, SystemChangeCommandError> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(invalid());
    };
    if bytes.len() > MAX_REQUEST_BYTES
        || bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err(invalid());
    }
    decode_request(bytes).map_err(Into::into)
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn blocking<T: Send + 'static>(
    op: impl FnOnce() -> Result<T, SystemChangeError> + Send + 'static,
) -> Result<T, SystemChangeCommandError> {
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|_| unavailable())?
        .map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewInput {
    change: SystemChange,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanInput {
    changes: Vec<SystemChange>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanIdInput {
    plan_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RollbackInput {
    entry_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmptyInput {}

fn plan_id(request: &tauri::ipc::Request<'_>) -> Result<String, SystemChangeCommandError> {
    let input: PlanIdInput = decode(request)?;
    if valid_id(&input.plan_id) {
        Ok(input.plan_id)
    } else {
        Err(SystemChangeError::PlanNotFound.into())
    }
}

#[tauri::command]
pub(crate) async fn preview_system_change(
    service: tauri::State<'_, Arc<SystemChangeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<PlannedChange, SystemChangeCommandError> {
    let input: PreviewInput = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.preview(input.change)).await
}

#[tauri::command]
pub(crate) async fn create_system_change_plan(
    service: tauri::State<'_, Arc<SystemChangeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<PlanTicket, SystemChangeCommandError> {
    let input: PlanInput = decode(&request)?;
    if input.changes.len() > MAX_PLAN_CHANGES {
        return Err(SystemChangeError::TooManyChanges.into());
    }
    let service = Arc::clone(service.inner());
    blocking(move || service.create_plan(input.changes)).await
}

/// Shows a native Windows dialog owned by the calling window. The webview
/// cannot answer it; the result is recorded in the plan store.
#[tauri::command]
pub(crate) async fn confirm_system_change_plan<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: tauri::State<'_, Arc<SystemChangeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), SystemChangeCommandError> {
    let plan_id = plan_id(&request)?;
    let owner = window
        .hwnd()
        .map_err(|_| SystemChangeCommandError::from(SystemChangeError::WindowUnavailable))?
        .0 as isize;
    let service = Arc::clone(service.inner());
    blocking(move || service.confirm_native(&plan_id, owner)).await
}

#[tauri::command]
pub(crate) async fn execute_system_change_plan(
    service: tauri::State<'_, Arc<SystemChangeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<ExecutionReport, SystemChangeCommandError> {
    let plan_id = plan_id(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.execute(&plan_id)).await
}

#[tauri::command]
pub(crate) async fn system_change_journal(
    service: tauri::State<'_, Arc<SystemChangeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<Vec<JournalView>, SystemChangeCommandError> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        service.reconcile_interrupted()?;
        service.journal()
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_system_rollback_plan(
    service: tauri::State<'_, Arc<SystemChangeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<PlanTicket, SystemChangeCommandError> {
    let input: RollbackInput = decode(&request)?;
    if input.entry_ids.is_empty()
        || input.entry_ids.len() > MAX_PLAN_CHANGES
        || !input.entry_ids.iter().all(|id| valid_id(id))
    {
        return Err(SystemChangeError::RollbackUnavailable.into());
    }
    let service = Arc::clone(service.inner());
    blocking(move || service.create_rollback_plan(&input.entry_ids)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_must_be_lowercase_hex_128_bit() {
        assert!(valid_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_id("0123456789ABCDEF0123456789ABCDEF"));
        assert!(!valid_id("0123456789abcdef"));
        assert!(!valid_id("../../../../../../../../etc/pass"));
    }

    #[test]
    fn inputs_reject_unknown_fields_and_free_form_commands() {
        assert!(decode_request::<PlanInput>(br#"{"changes":[],"extra":1}"#).is_err());
        assert!(
            decode_request::<PreviewInput>(
                br#"{"change":{"kind":"runCommand","command":"whoami"}}"#
            )
            .is_err()
        );
        assert!(
            decode_request::<PreviewInput>(
                br#"{"change":{"kind":"setHibernation","enabled":false}}"#
            )
            .is_ok()
        );
    }
}
