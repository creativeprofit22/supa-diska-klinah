mod build_artifacts;
mod execution;
mod filesystem;
mod preview;
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
    preview_temporary_caches,
};
pub(crate) use storage::CleanupStorage;
pub use storage::{
    ArtifactBudgetPolicy, AutoCleanupPolicy, BuildEcosystem, BuildProfile, CleanupDisposition,
    ProjectBudgetOverride, ProjectBudgetPolicy, ProjectRoot, RegisteredArtifactPath,
};
