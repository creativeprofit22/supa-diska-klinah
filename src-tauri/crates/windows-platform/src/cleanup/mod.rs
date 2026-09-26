mod build_artifacts;
mod execution;
mod filesystem;
mod preview;
pub(crate) use preview::current_protection;
mod recycle;
mod storage;

pub use build_artifacts::{
    ArtifactBudgetAnalysisStatus, ArtifactBudgetPreview, BuildArtifactError, BuildRun,
    BuildRunState, RegisterBuildProfileInput, SetArtifactBudgetPolicyResult,
};
pub use execution::{
    CleanupExecutionSummary, CleanupItemOutcome, CleanupPlanSummary, CleanupService,
    CleanupServiceError, ProjectArtifactScan,
};
pub(crate) use filesystem::IdentityGuard;
pub use filesystem::WindowsFileSystem;
pub use preview::{
    CleanupPreview, CleanupPreviewError, ProjectArtifactDiscovery, discover_project_artifacts,
    discover_project_artifacts_with_workers, preview_temporary_caches,
};
pub use storage::{
    ArtifactBudgetPolicy, AutoCleanupPolicy, BuildEcosystem, BuildProfile, CleanupDisposition,
    ProjectBudgetOverride, ProjectBudgetPolicy, ProjectRoot, RegisteredArtifactPath,
};
pub(crate) use storage::{CleanupStorage, StorageError, read_json, write_json};
