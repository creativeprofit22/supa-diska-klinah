//! Sole vendor process owner. Registry-derived commands only; never a helper operation.
//! ShellExecuteEx uses a dedicated STA thread, visible vendor UI, and no forced termination.
//! Unsupported syntax fails closed rather than guessing Windows executable tokenization.
use super::uninstaller::{InventoryError, RegistryLocation, RegistryReader, text};
pub use super::vendor_jobs::{VendorJob, VendorJobError, VendorJobManager, VendorJobState};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows_sys::Win32::{
    System::Registry::REG_DWORD, System::SystemInformation::GetSystemDirectoryW,
};

const MAX_COMMAND_UNITS: usize = 8192;
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_UNITS: usize = 1024;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct VendorCommand {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub msi: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandError {
    Registry,
    UnsupportedSyntax,
    UnsupportedExecutable,
    InvalidProductCode,
    SystemDirectory,
}
impl From<InventoryError> for CommandError {
    fn from(_: InventoryError) -> Self {
        Self::Registry
    }
}

/// Re-read the exact opaque-ID-resolved registry location. The caller must repeat resolution
/// at confirmation and immediately before launch, and compare against confirmed executable identity.
pub(crate) fn resolve(
    reader: &dyn RegistryReader,
    location: &RegistryLocation,
) -> Result<VendorCommand, CommandError> {
    let msi = reader.value(location, "WindowsInstaller")?;
    match msi {
        Some(value) if value.kind == REG_DWORD && value.bytes == 1u32.to_le_bytes() => {
            msi_command(&location.subkey, system_directory()?)
        }
        Some(value) if value.kind != REG_DWORD || value.bytes != 0u32.to_le_bytes() => {
            Err(CommandError::Registry)
        }
        _ => {
            // QuietUninstallString is intentionally ignored: vendor UI belongs to the vendor.
            let command =
                text(reader.value(location, "UninstallString")?)?.ok_or(CommandError::Registry)?;
            parse(&command)
        }
    }
}

fn system_directory() -> Result<PathBuf, CommandError> {
    let mut buffer = [0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() || buffer[..length].contains(&0) {
        return Err(CommandError::SystemDirectory);
    }
    let path = String::from_utf16(&buffer[..length]).map_err(|_| CommandError::SystemDirectory)?;
    if !absolute_executable_path(&format!("{path}\\msiexec.exe")) {
        return Err(CommandError::SystemDirectory);
    }
    Ok(PathBuf::from(path))
}
fn product_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 38
        && bytes[0] == b'{'
        && bytes[37] == b'}'
        && bytes[1..37].iter().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
fn msi_command(guid: &str, trusted_system: PathBuf) -> Result<VendorCommand, CommandError> {
    if !product_code(guid) {
        return Err(CommandError::InvalidProductCode);
    }
    Ok(VendorCommand {
        executable: trusted_system.join("msiexec.exe"),
        arguments: vec!["/x".into(), guid.to_ascii_uppercase(), "/norestart".into()],
        msi: true,
    })
}
fn absolute_executable_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 7
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'\\'
        && !value[2..].contains([':', '/', '%', '\0', '*', '?', '"', '<', '>', '|'])
        && value[3..].split('\\').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' '])
        })
        && value.to_ascii_lowercase().ends_with(".exe")
}

/// Accept only an intentionally small, unambiguous subset of Windows argv syntax.
/// Quotes must surround a whole token; embedded quotes, escaped quotes, backslash-before-quote,
/// controls and expansion are unsupported. Accepted tokens round-trip with Windows quoting.
pub(crate) fn parse(command: &str) -> Result<VendorCommand, CommandError> {
    if command.is_empty()
        || command.encode_utf16().count() > MAX_COMMAND_UNITS
        || command.starts_with(char::is_whitespace)
        || command.ends_with(char::is_whitespace)
        || command
            .chars()
            .any(|c| c.is_control() || matches!(c, '%' | '`' | '|' | '&' | '<' | '>'))
    {
        return Err(CommandError::UnsupportedSyntax);
    }
    let mut tokens = Vec::new();
    let mut remaining = command;
    while !remaining.is_empty() {
        if tokens.len() > MAX_ARGUMENTS {
            return Err(CommandError::UnsupportedSyntax);
        }
        let (token, rest) = if let Some(quoted) = remaining.strip_prefix('"') {
            let end = quoted.find('"').ok_or(CommandError::UnsupportedSyntax)?;
            let token = &quoted[..end];
            let rest = &quoted[end + 1..];
            if token.ends_with('\\') || (!rest.is_empty() && !rest.starts_with(' ')) {
                return Err(CommandError::UnsupportedSyntax);
            }
            (token, rest)
        } else {
            let end = remaining.find(' ').unwrap_or(remaining.len());
            let token = &remaining[..end];
            if token.contains('"') {
                return Err(CommandError::UnsupportedSyntax);
            }
            (token, &remaining[end..])
        };
        if token.encode_utf16().count() > MAX_ARGUMENT_UNITS {
            return Err(CommandError::UnsupportedSyntax);
        }
        tokens.push(token.to_owned());
        remaining = rest.trim_start_matches(' ');
    }
    let executable = tokens.remove(0);
    if !absolute_executable_path(&executable) {
        return Err(CommandError::UnsupportedExecutable);
    }
    let name = executable.rsplit('\\').next().unwrap().to_ascii_lowercase();
    // Executable identity/native PE validation is still mandatory before any launch. This rejects
    // known command hosts; an executable filename is not a publisher/signature trust assertion.
    if [
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
        "wscript.exe",
        "cscript.exe",
        "mshta.exe",
        "rundll32.exe",
        "regsvr32.exe",
        "msiexec.exe",
        "bash.exe",
        "sh.exe",
        "wsl.exe",
        "python.exe",
        "pythonw.exe",
        "py.exe",
        "node.exe",
        "java.exe",
        "javaw.exe",
        "perl.exe",
        "ruby.exe",
        "reg.exe",
        "installutil.exe",
        "msbuild.exe",
    ]
    .contains(&name.as_str())
    {
        return Err(CommandError::UnsupportedExecutable);
    }
    Ok(VendorCommand {
        executable: executable.into(),
        arguments: tokens,
        msi: false,
    })
}

// API contracts inspected: Microsoft ShellExecuteExW and SHELLEXECUTEINFOW documentation.
// NOASYNC is required because this dedicated STA does not run a message loop.
use crate::cleanup::{IdentityGuard, WindowsFileSystem};
use cleanup_core::storage::{ObservedEntry, RootAuthorization};
use cleanup_core::{EntryKind, FileSystem};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::GetBinaryTypeW,
    System::{
        Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
        Threading::{GetExitCodeProcess, WaitForSingleObject},
    },
    UI::{
        Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutableEvidence {
    pub root: RootAuthorization,
    pub entry: ObservedEntry,
    pub digest: [u8; 32],
}

pub(crate) trait VendorProcess {
    fn wait(&mut self, cancel: &AtomicBool, timeout: Duration) -> Option<u32>;
}
pub(crate) enum LaunchResult {
    Process(Box<dyn VendorProcess>),
    NotStarted,
    Cancelled,
    Failed(u32),
    Unknown,
}
pub(crate) trait ProcessBoundary: Send + Sync {
    fn validate(&self, command: &VendorCommand) -> Result<ExecutableEvidence, VendorJobError>;
    fn launch(
        &self,
        command: &VendorCommand,
        evidence: &ExecutableEvidence,
        cancel: &AtomicBool,
    ) -> LaunchResult;
}
pub(crate) struct NativeProcessBoundary;

fn executable_guard(
    command: &VendorCommand,
    expected: Option<&ExecutableEvidence>,
) -> Result<(ExecutableEvidence, IdentityGuard), VendorJobError> {
    use sha2::{Digest, Sha256};
    let fail = || VendorJobError::ExecutableChanged;
    let fs = WindowsFileSystem;
    let parent = command.executable.parent().ok_or_else(fail)?;
    // Reuse the broker's held-handle path/identity validation, without its elevation behavior.
    let validated = crate::security::path_policy::validate_executable(parent, &command.executable)
        .map_err(|_| fail())?;
    let canonical = validated.as_path().to_path_buf();
    let root_path = canonical.parent().ok_or_else(fail)?.to_path_buf();
    let root_meta = fs.metadata_no_follow(&root_path).map_err(|_| fail())?;
    let meta = fs.metadata_no_follow(&canonical).map_err(|_| fail())?;
    if meta.kind != EntryKind::File || meta.size > 128 * 1024 * 1024 || meta.size < 256 {
        return Err(fail());
    }
    let root = RootAuthorization {
        snapshot_id: "00000000000000000000000000000000".into(),
        root_id: "00000000000000000000000000000001".into(),
        canonical_path: root_path,
        identity: root_meta.identity.ok_or_else(fail)?,
    };
    let entry = ObservedEntry {
        canonical_path: canonical,
        identity: meta.identity.ok_or_else(fail)?,
        kind: meta.kind,
        logical_bytes: meta.size,
        allocated_bytes: None,
        modified_unix_nanos: meta
            .modified
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u64::try_from(d.as_nanos()).ok())
            .ok_or_else(fail)?,
    };
    let guard = fs.guard_entry(&root, &entry, true).map_err(|_| fail())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut offset = 0;
    while offset < meta.size {
        let count = guard.read_at(&mut buffer, offset).map_err(|_| fail())?;
        if count == 0 {
            return Err(fail());
        }
        hasher.update(&buffer[..count]);
        offset += count as u64;
    }
    // GetBinaryType rejects scripts/DOS/16-bit images; inspect PE headers too, rejecting DLLs
    // and managed CLR launchers. Filename extension and identity are not signature trust.
    let path: Vec<u16> = command
        .executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut kind = 0;
    if unsafe { GetBinaryTypeW(path.as_ptr(), &mut kind) } == 0 || !matches!(kind, 0 | 6) {
        return Err(fail());
    }
    let count = guard.read_at(&mut buffer, 0).map_err(|_| fail())?;
    let bytes = &buffer[..count];
    if bytes.get(..2) != Some(b"MZ") {
        return Err(fail());
    }
    let pe = u32::from_le_bytes(
        bytes
            .get(60..64)
            .ok_or_else(fail)?
            .try_into()
            .map_err(|_| fail())?,
    ) as usize;
    let header = bytes
        .get(pe..pe.checked_add(256).ok_or_else(fail)?)
        .ok_or_else(fail)?;
    if &header[..4] != b"PE\0\0" || u16::from_le_bytes([header[22], header[23]]) & 0x2000 != 0 {
        return Err(fail());
    }
    let magic = u16::from_le_bytes([header[24], header[25]]);
    let clr = match magic {
        0x10b => 24 + 96 + 14 * 8,
        0x20b => 24 + 112 + 14 * 8,
        _ => return Err(fail()),
    };
    if header[clr..clr + 8].iter().any(|b| *b != 0) {
        return Err(fail());
    }
    let evidence = ExecutableEvidence {
        root,
        entry,
        digest: hasher.finalize().into(),
    };
    if expected.is_some_and(|expected| expected != &evidence) {
        return Err(fail());
    }
    Ok((evidence, guard))
}
use std::os::windows::ffi::OsStrExt;

struct ComApartment;
impl ComApartment {
    fn initialize() -> Option<Self> {
        (unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        } >= 0)
            .then_some(Self)
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct NativeVendorProcess(HANDLE);
impl Drop for NativeVendorProcess {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl VendorProcess for NativeVendorProcess {
    fn wait(&mut self, cancel: &AtomicBool, timeout: Duration) -> Option<u32> {
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::Acquire) || start.elapsed() >= timeout {
                return None;
            }
            match unsafe { WaitForSingleObject(self.0, 50) } {
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    return (unsafe { GetExitCodeProcess(self.0, &mut code) } != 0).then_some(code);
                }
                WAIT_TIMEOUT => {}
                _ => return None,
            }
        }
    }
}
impl ProcessBoundary for NativeProcessBoundary {
    fn validate(&self, command: &VendorCommand) -> Result<ExecutableEvidence, VendorJobError> {
        executable_guard(command, None).map(|(evidence, _)| evidence)
    }
    fn launch(
        &self,
        command: &VendorCommand,
        evidence: &ExecutableEvidence,
        cancel: &AtomicBool,
    ) -> LaunchResult {
        let Some(_com) = ComApartment::initialize() else {
            return LaunchResult::Failed(0);
        };
        let Ok((_evidence, _guard)) = executable_guard(command, Some(evidence)) else {
            return LaunchResult::Failed(0);
        };
        let wide = |value: &str| value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let file: Vec<u16> = command
            .executable
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Accepted tokens contain no quotes. Double terminal backslashes when quoting.
        let parameters = wide(
            &command
                .arguments
                .iter()
                .map(|arg| {
                    let trailing = arg.chars().rev().take_while(|c| *c == '\\').count();
                    format!("\"{arg}{}\"", "\\".repeat(trailing))
                })
                .collect::<Vec<_>>()
                .join(" "),
        );
        let verb = wide("open");
        let directory: Vec<u16> = command
            .executable
            .parent()
            .unwrap()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.lpDirectory = directory.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        // Final cancellation gate after hashing/identity validation, immediately before launch.
        if cancel.load(Ordering::Acquire) {
            return LaunchResult::NotStarted;
        }
        let success = unsafe { ShellExecuteExW(&mut info) };
        let error = if success == 0 {
            unsafe { GetLastError() }
        } else {
            0
        };
        // All path/file guards end on return, BEFORE waiting: vendor may remove its own root.
        if success == 0 {
            if !info.hProcess.is_null() {
                unsafe {
                    CloseHandle(info.hProcess);
                }
            }
            if error == 1223 {
                LaunchResult::Cancelled
            } else {
                LaunchResult::Failed(error)
            }
        } else if info.hProcess.is_null() {
            LaunchResult::Unknown
        } else {
            LaunchResult::Process(Box::new(NativeVendorProcess(info.hProcess)))
        }
    }
}

#[cfg(test)]
pub(super) fn compile_vendor_fixture(destination: &std::path::Path) {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/vendor-disposable.rs");
    assert!(
        std::process::Command::new("rustc")
            .arg(source)
            .arg("--edition=2024")
            .arg("-o")
            .arg(destination)
            .status()
            .unwrap()
            .success()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ambiguous_and_interpreter_commands() {
        for command in [
            "",
            " uninstall.exe",
            "uninstall.exe",
            r"C:uninstall.exe",
            r"C:\Program Files\Vendor\uninstall.exe /u",
            r#""C:\Vendor\uninstall.exe"suffix"#,
            r#""C:\Vendor\uninstall.exe" "unterminated"#,
            r#""C:\Vendor\uninstall.exe" "ends\""#,
            r"C:\Vendor\uninstall.exe %TEMP%",
            r"C:\Windows\System32\cmd.exe /c erase",
            r"C:\Windows\System32\msiexec.exe /I{abc}",
            r"\\server\share\uninstall.exe",
            r"C:\vendor\..\uninstall.exe",
            r"C:\vendor\uninstall.exe:other.exe",
            r"C:\vendor\uninstall.cmd",
            r"C:\vendor\*.exe",
            r"C:\vendor\uninstall?.exe",
        ] {
            assert!(parse(command).is_err(), "{command}");
        }
        assert!(parse(&format!(r"C:\Vendor\uninstall.exe {}", "a".repeat(1025))).is_err());
        assert!(
            parse(&format!(
                r"C:\Vendor\uninstall.exe {}",
                vec!["a"; 65].join(" ")
            ))
            .is_err()
        );
    }
    #[test]
    fn accepts_bounded_whole_token_quotes() {
        let command = parse(r#""C:\Program Files\Vendor\uninstall.exe" /remove "a b" """#).unwrap();
        assert_eq!(command.arguments, ["/remove", "a b", ""]);
        assert!(!command.msi);
    }
    #[test]
    fn msi_synthesizes_only_strict_product_guid() {
        let guid = "{01234567-89ab-cdef-0123-456789abcdef}";
        let command = msi_command(guid, PathBuf::from(r"C:\Windows\System32")).unwrap();
        assert_eq!(
            command.arguments,
            ["/x", &guid.to_ascii_uppercase(), "/norestart"]
        );
        assert!(command.msi);
        for invalid in [
            "{abc}",
            "01234567-89ab-cdef-0123-456789abcdef",
            "{01234567-89ab-cdef-0123-456789abcdeg}",
            "{01234567-89ab-cdef-0123-456789abcdef} /quiet",
        ] {
            assert!(!product_code(invalid));
        }
    }
}
