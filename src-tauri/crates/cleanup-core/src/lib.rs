mod engine;
mod filesystem;
mod protection;
mod rules;
mod scanner;

pub use engine::*;
pub use filesystem::*;
pub use protection::*;
pub use rules::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    pub platform: String,
    pub architecture: String,
    pub adapter_ready: bool,
}

impl FoundationStatus {
    pub fn ready(platform: impl Into<String>, architecture: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            architecture: architecture.into(),
            adapter_ready: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FoundationStatus;

    #[test]
    fn ready_status_preserves_adapter_identity() {
        let status = FoundationStatus::ready("windows", "x86_64");

        assert_eq!(status.platform, "windows");
        assert_eq!(status.architecture, "x86_64");
        assert!(status.adapter_ready);
    }
}
