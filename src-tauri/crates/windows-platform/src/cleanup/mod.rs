mod execution;
mod filesystem;
mod preview;
mod recycle;
mod storage;

pub use execution::{
    CleanupExecutionSummary, CleanupItemOutcome, CleanupPlanSummary, CleanupService,
    CleanupServiceError,
};
pub use filesystem::WindowsFileSystem;
pub use preview::{CleanupPreview, CleanupPreviewError, preview_temporary_caches};
pub use storage::{AutoCleanupPolicy, CleanupDisposition};
