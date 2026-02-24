//! Shared IPC types for communication between daemon and app.
//!
//! These types are the canonical wire format for JSON-RPC responses.
//! Both the daemon (serializer) and app (deserializer) use these directly.

mod config;
mod diff;
mod events;
mod types;

pub use config::{
    AgentConfigInfo, GetConfigResult, GlobalConfigInfo, GlobalConfigUpdate, RepoConfigInfo,
    RepoConfigUpdate, StorageConfigInfo, StorageConfigUpdate, SyncConfigInfo, SyncConfigUpdate,
    UpdateConfigResult, WorkflowFileInfo,
};

pub use diff::{
    ApproveIntentResult, CommitDiffInfo, DiffHunkInfo, GetRunDiffResult, IntentDiffResult,
    IntentTourResult, LineAnnotationInfo, RejectIntentResult, TourInfo, TourStepInfo,
};

pub use events::AirlockEvent;

pub use types::{
    ApplyPatchesResult, ApproveStepResult, ArtifactInfo, JobResultInfo, PatchError, RunInfo,
    StepResultInfo,
};
