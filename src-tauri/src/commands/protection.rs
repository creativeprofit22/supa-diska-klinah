// Local-first protection commands (ADR 0003). The webview sends fixed enums,
// booleans and opaque 32-hex IDs only. Folders come from the native picker;
// destructive actions are confirmed in native dialogs the webview cannot answer.

use std::sync::Arc;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use windows_platform::i18n::native;
use windows_platform::protection::{
    ProtectionNetworkPolicy, Zeroizing,
    breach::PasswordBreachResult,
    decode_request,
    defender_history::DefenderHistory,
    native_ui,
    process::ProcessInventory,
    quarantine::QuarantineEntry,
    rules_store::RulesStatus,
    service::{
        ProtectionError, ProtectionOverview, ProtectionService, ProtectionSettings, ScanReport,
        ScanScope, ScanStatus,
    },
};

const MAX_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtectionCommandError {
    code: ProtectionError,
    message: String,
}

impl From<ProtectionError> for ProtectionCommandError {
    fn from(code: ProtectionError) -> Self {
        let message = code.to_string();
        Self { code, message }
    }
}

type Service<'a> = tauri::State<'a, Arc<ProtectionService>>;
type CommandResult<T> = Result<T, ProtectionCommandError>;

fn decode<T: DeserializeOwned>(request: &tauri::ipc::Request<'_>) -> CommandResult<T> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(ProtectionError::InvalidInput.into());
    };
    decode_bytes(bytes)
}

fn decode_bytes<T: DeserializeOwned>(bytes: &[u8]) -> CommandResult<T> {
    if bytes.len() > MAX_REQUEST_BYTES
        || bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err(ProtectionError::InvalidInput.into());
    }
    decode_request(bytes).map_err(Into::into)
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

async fn blocking<T: Send + 'static>(
    op: impl FnOnce() -> Result<T, ProtectionError> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|_| ProtectionCommandError::from(ProtectionError::Storage))?
        .map_err(Into::into)
}

fn owner<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> CommandResult<isize> {
    Ok(window
        .hwnd()
        .map_err(|_| ProtectionCommandError::from(ProtectionError::WindowUnavailable))?
        .0 as isize)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmptyInput {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdInput {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScanInput {
    scope: ScanScope,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyInput {
    network: ProtectionNetworkPolicy,
    amsi_enabled: bool,
}

/// The password never appears in logs: Debug is not derived.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PasswordInput {
    password: String,
}

fn id_input(request: &tauri::ipc::Request<'_>) -> CommandResult<String> {
    let IdInput { id } = decode(request)?;
    if valid_id(&id) {
        Ok(id)
    } else {
        Err(ProtectionError::InvalidInput.into())
    }
}

#[tauri::command]
pub(crate) async fn protection_overview(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ProtectionOverview> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.overview()).await
}

#[tauri::command]
pub(crate) async fn set_protection_network_policy(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ProtectionSettings> {
    let input: PolicyInput = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.set_network_policy(input.network, input.amsi_enabled)).await
}

#[tauri::command]
pub(crate) async fn list_running_programs(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ProcessInventory> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.processes()).await
}

/// Quick scope scans fixed native locations. Folder scope opens the native
/// picker; cancelling it returns `Cancelled`.
#[tauri::command]
pub(crate) async fn start_protection_scan<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ScanReport> {
    let ScanInput { scope } = decode(&request)?;
    let owner = owner(&window)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        let folder = match scope {
            ScanScope::Quick => None,
            ScanScope::Folder => {
                Some(native_ui::pick_scan_folder(owner)?.ok_or(ProtectionError::Cancelled)?)
            }
        };
        service.scan(scope, folder)
    })
    .await
}

#[tauri::command]
pub(crate) async fn cancel_protection_scan(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<()> {
    let EmptyInput {} = decode(&request)?;
    service.inner().cancel_scan().map_err(Into::into)
}

/// Lets the Scan page reattach to a scan that outlived its component.
#[tauri::command]
pub(crate) async fn protection_scan_status(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ScanStatus> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.scan_status()).await
}

#[tauri::command]
pub(crate) async fn last_protection_scan(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<Option<ScanReport>> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.last_scan()).await
}

#[tauri::command]
pub(crate) async fn quarantine_protection_finding<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<QuarantineEntry> {
    let id = id_input(&request)?;
    let owner = owner(&window)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        let text = native();
        native_ui::confirm(
            owner,
            text.quarantine_file_title,
            &service.quarantine_prompt(&id, text)?,
        )?;
        service.quarantine_finding(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_quarantine(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<Vec<QuarantineEntry>> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || Ok(service.quarantine_list())).await
}

#[tauri::command]
pub(crate) async fn restore_quarantined<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<String> {
    let id = id_input(&request)?;
    let owner = owner(&window)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        let text = native();
        native_ui::confirm(
            owner,
            text.restore_file_title,
            &service.restore_prompt(&id, text)?,
        )?;
        service.restore(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn delete_quarantined<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<()> {
    let id = id_input(&request)?;
    let owner = owner(&window)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        let text = native();
        native_ui::confirm(
            owner,
            text.delete_quarantined_title,
            &service.delete_prompt(&id, text)?,
        )?;
        service.delete(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn allow_protection_finding(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ProtectionSettings> {
    let id = id_input(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.allow_hash(&id)).await
}

#[tauri::command]
pub(crate) async fn clear_protection_allowlist(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<ProtectionSettings> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.clear_allowlist()).await
}

#[tauri::command]
pub(crate) async fn import_rule_pack<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<RulesStatus> {
    let EmptyInput {} = decode(&request)?;
    let owner = owner(&window)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        let folder = native_ui::pick_rule_pack_folder(owner)?.ok_or(ProtectionError::Cancelled)?;
        service.import_rules(&folder)
    })
    .await
}

#[tauri::command]
pub(crate) async fn restore_previous_rule_pack<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<RulesStatus> {
    let EmptyInput {} = decode(&request)?;
    let owner = owner(&window)?;
    let service = Arc::clone(service.inner());
    blocking(move || {
        let text = native();
        native_ui::confirm(
            owner,
            text.restore_previous_rule_pack_title,
            text.restore_previous_rule_pack_body,
        )?;
        service.restore_previous_rules()
    })
    .await
}

#[tauri::command]
pub(crate) async fn download_rule_pack(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<RulesStatus> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || service.download_rules()).await
}

#[tauri::command]
pub(crate) async fn check_password_breach(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<PasswordBreachResult> {
    let input: PasswordInput = decode(&request)?;
    let password = Zeroizing::new(input.password.into_bytes());
    let service = Arc::clone(service.inner());
    blocking(move || service.check_password(password)).await
}

#[tauri::command]
pub(crate) async fn get_defender_history(
    service: Service<'_>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<DefenderHistory> {
    let EmptyInput {} = decode(&request)?;
    let service = Arc::clone(service.inner());
    blocking(move || Ok(service.defender_history())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_must_be_lowercase_hex_128_bit() {
        assert!(valid_id("0123456789abcdef0123456789abcdef"));
        for bad in [
            "0123456789ABCDEF0123456789ABCDEF",
            "0123",
            "..\\..\\..\\..\\..\\..\\..\\..\\x",
            "",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn inputs_are_object_shaped_bounded_and_closed() {
        assert!(decode_bytes::<EmptyInput>(b"{}").is_ok());
        assert!(decode_bytes::<EmptyInput>(b"[]").is_err());
        assert!(decode_bytes::<EmptyInput>(br#"{"path":"C:\\"}"#).is_err());
        assert!(decode_bytes::<ScanInput>(br#"{"scope":"quick"}"#).is_ok());
        assert!(decode_bytes::<ScanInput>(br#"{"scope":"folder","path":"C:\\"}"#).is_err());
        assert!(decode_bytes::<ScanInput>(br#"{"scope":"everything"}"#).is_err());
        assert!(decode_bytes::<PolicyInput>(br#"{"network":{"ruleDownload":true,"passwordBreachCheck":false},"amsiEnabled":false}"#).is_ok());
        assert!(decode_bytes::<PolicyInput>(br#"{"network":{"ruleDownload":true,"passwordBreachCheck":false,"url":"http://x"},"amsiEnabled":false}"#).is_err());
        let huge = format!(r#"{{"password":"{}"}}"#, "a".repeat(MAX_REQUEST_BYTES));
        assert!(decode_bytes::<PasswordInput>(huge.as_bytes()).is_err());
    }

    #[test]
    fn errors_serialize_with_stable_codes() {
        let error = ProtectionCommandError::from(ProtectionError::NetworkDisabled);
        assert_eq!(error.code, ProtectionError::NetworkDisabled);
        assert_eq!(error.message, "this network feature is turned off");
        let error = ProtectionCommandError::from(ProtectionError::Quarantine(
            windows_platform::protection::quarantine::QuarantineError::Collision,
        ));
        assert!(error.message.contains("nothing was overwritten"));
    }
}
