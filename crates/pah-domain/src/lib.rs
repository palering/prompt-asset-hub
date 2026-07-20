//! Core domain model for Prompt Asset Hub.
//!
//! This crate deliberately has no knowledge of databases, Tauri, HTTP, or
//! concrete file systems. Constructors and state-transition methods encode the
//! invariants that every adapter must preserve.

mod asset;
mod common;
mod error;
mod evaluation;
mod generation;
mod ids;
mod project;
mod prompt;
mod tenant;

pub use asset::{
    Asset, AssetFile, AssetFileRole, AssetLink, AssetLocation, AssetRelationType, AssetStatus,
    AssetVisibility, MediaType,
};
pub use common::{JsonObject, RecordTimestamps};
pub use error::DomainError;
pub use evaluation::Evaluation;
pub use generation::{
    GenerationCost, GenerationRun, GenerationSource, GenerationStatus, InputSnapshot,
};
pub use ids::{
    AssetFileId, AssetId, AssetLinkId, EvaluationId, GenerationRunId, ProjectId, PromptId,
    PromptVariantId, TagId, TenantId,
};
pub use project::{Project, ProjectStatus};
pub use prompt::{Prompt, PromptStatus, PromptVariant};
pub use tenant::{Tenant, TenantStatus};
