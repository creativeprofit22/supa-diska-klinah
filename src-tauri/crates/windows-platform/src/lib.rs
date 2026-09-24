#[cfg(not(target_os = "windows"))]
compile_error!("windows-platform supports Windows targets only");

pub mod cleanup;
pub mod drivers;
pub mod firewall;
pub mod history;
pub mod hosts;
pub mod optimizer;
pub mod os_info;
pub mod power;
pub mod privacy;
pub mod privilege;
pub mod protection;
pub mod restore;
pub mod scheduler;
pub mod security;
pub mod services;
mod startup;
pub mod startup_items;
pub mod storage;
pub mod system_change;
pub mod updates;
pub mod win_registry;

pub use cleanup::WindowsFileSystem;
pub use cleanup_core::FoundationStatus;
pub use startup::{StartupWindowMode, startup_window_mode};

pub fn foundation_status() -> FoundationStatus {
    FoundationStatus::ready("windows", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::foundation_status;

    #[test]
    fn reports_the_native_windows_adapter() {
        let status = foundation_status();

        assert_eq!(status.platform, "windows");
        assert_eq!(status.architecture, std::env::consts::ARCH);
        assert!(status.adapter_ready);
    }
}
