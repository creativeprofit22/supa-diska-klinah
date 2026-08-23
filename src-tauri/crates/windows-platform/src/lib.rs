#[cfg(not(target_os = "windows"))]
compile_error!("windows-platform supports Windows targets only");

pub use cleanup_core::FoundationStatus;

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
