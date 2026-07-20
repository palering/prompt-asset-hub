use std::collections::BTreeMap;

use async_trait::async_trait;
use pah_domain::{
    Asset, AssetFile, AssetFileRole, AssetId, AssetLink, Evaluation, GenerationRun, MediaType,
    Project, ProjectId, Prompt, PromptId, PromptStatus, PromptVariant, PromptVariantId, TenantId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepositoryError {
    #[error("repository conflict: {0}")]
    Conflict(String),
    #[error("repository is unavailable: {0}")]
    Unavailable(String),
    #[error("repository returned corrupt data: {0}")]
    CorruptData(String),
    #[error("repository operation is not supported: {0}")]
    Unsupported(String),
    #[error("repository failure: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlobStoreError {
    #[error("staged blob was not found: {0}")]
    StagedBlobNotFound(String),
    #[error("blob type or contents are invalid: {0}")]
    InvalidBlob(String),
    #[error("blob store is unavailable: {0}")]
    Unavailable(String),
    #[error("blob store failure: {0}")]
    Other(String),
}

/// A safe reference produced by an adapter-controlled staging operation.
///
/// Keeping raw file paths out of the application DTO prevents UI callers from
/// turning asset import into an arbitrary file-read primitive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedBlob {
    pub token: String,
    pub original_filename: Option<String>,
    pub declared_mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBlobRequest {
    pub asset_id: AssetId,
    pub staged: StagedBlob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    pub storage_key: String,
    pub detected_mime_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub derivatives: Vec<StoredDerivative>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDerivative {
    pub role: AssetFileRole,
    pub storage_key: String,
    pub detected_mime_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub generator_version: Option<String>,
}

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn store(
        &self,
        tenant_id: TenantId,
        request: StoreBlobRequest,
    ) -> Result<StoredBlob, BlobStoreError>;

    /// Deletes a blob created by `store`. Implementations must be idempotent.
    async fn delete(&self, tenant_id: TenantId, storage_key: &str) -> Result<(), BlobStoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    Prompt,
    Asset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchCriteria {
    pub query: Option<String>,
    pub kinds: Vec<SearchKind>,
    pub project_id: Option<ProjectId>,
    pub providers: Vec<String>,
    pub models: Vec<String>,
    pub media_types: Vec<MediaType>,
    pub prompt_statuses: Vec<PromptStatus>,
    pub minimum_score: Option<u8>,
    pub featured_only: bool,
    pub tags: Vec<String>,
    pub include_archived: bool,
    pub limit: u32,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SearchRecordId {
    Prompt(PromptId),
    Asset(AssetId),
}

/// A database-friendly projection for the mixed prompt/asset result list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchRecord {
    /// Kept in the repository projection so the application can reject a
    /// broken adapter that accidentally returns another tenant's row.
    pub tenant_id: TenantId,
    pub id: SearchRecordId,
    pub title: String,
    pub summary: Option<String>,
    pub project_id: Option<ProjectId>,
    pub project_name: Option<String>,
    pub media_type: Option<MediaType>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thumbnail_key: Option<String>,
    pub overall_score: Option<u8>,
    pub is_featured: bool,
    pub tags: Vec<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptDetailSnapshot {
    pub project: Project,
    pub prompt: Prompt,
    pub variants: Vec<PromptVariant>,
    pub generation_runs: Vec<GenerationRun>,
    pub assets: Vec<Asset>,
    pub evaluations: Vec<Evaluation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetDetailSnapshot {
    pub asset: Asset,
    pub links: Vec<AssetLink>,
    pub prompts: Vec<Prompt>,
    pub variants: Vec<PromptVariant>,
    pub generation_runs: Vec<GenerationRun>,
    pub evaluations: Vec<Evaluation>,
    pub related_assets: Vec<Asset>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetContextReplacement {
    pub asset_id: AssetId,
    pub generation_run: Option<GenerationRun>,
    pub asset_link: Option<AssetLink>,
    pub evaluation: Option<Evaluation>,
    pub replaced_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryOverviewSnapshot {
    pub projects: Vec<Project>,
    pub prompts: Vec<Prompt>,
    pub variants: Vec<PromptVariant>,
    pub variant_result_counts: BTreeMap<PromptVariantId, u64>,
}

/// Persistence operations are shaped around application use cases. Aggregate
/// writes must be atomic in the adapter implementation.
#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn create_project(
        &self,
        tenant_id: TenantId,
        project: Project,
    ) -> Result<(), RepositoryError>;

    async fn project_exists(
        &self,
        tenant_id: TenantId,
        project_id: ProjectId,
    ) -> Result<bool, RepositoryError>;

    async fn create_prompt_bundle(
        &self,
        tenant_id: TenantId,
        prompt: Prompt,
        initial_variant: Option<PromptVariant>,
    ) -> Result<(), RepositoryError>;

    async fn prompt_exists(
        &self,
        tenant_id: TenantId,
        prompt_id: PromptId,
    ) -> Result<bool, RepositoryError>;

    async fn add_prompt_variant(
        &self,
        tenant_id: TenantId,
        variant: PromptVariant,
    ) -> Result<(), RepositoryError>;

    async fn get_prompt_variant(
        &self,
        tenant_id: TenantId,
        variant_id: PromptVariantId,
    ) -> Result<Option<PromptVariant>, RepositoryError>;

    async fn create_asset(
        &self,
        tenant_id: TenantId,
        asset: Asset,
        files: Vec<AssetFile>,
    ) -> Result<(), RepositoryError>;

    /// Returns the subset of IDs that exist and belong to `tenant_id`.
    async fn existing_asset_ids(
        &self,
        tenant_id: TenantId,
        asset_ids: &[AssetId],
    ) -> Result<Vec<AssetId>, RepositoryError>;

    async fn record_generation(
        &self,
        tenant_id: TenantId,
        generation_run: GenerationRun,
        asset_links: Vec<AssetLink>,
    ) -> Result<(), RepositoryError>;

    async fn create_evaluation(
        &self,
        tenant_id: TenantId,
        evaluation: Evaluation,
    ) -> Result<(), RepositoryError>;

    async fn replace_asset_tags(
        &self,
        tenant_id: TenantId,
        asset_id: AssetId,
        tags: Vec<String>,
        updated_at: OffsetDateTime,
    ) -> Result<(), RepositoryError>;

    /// Replaces the active editable context while preserving prior generation
    /// runs and evaluations as soft-deleted history.
    async fn replace_asset_context(
        &self,
        tenant_id: TenantId,
        replacement: AssetContextReplacement,
    ) -> Result<(), RepositoryError>;

    async fn search_library(
        &self,
        tenant_id: TenantId,
        criteria: SearchCriteria,
    ) -> Result<SearchPage<SearchRecord>, RepositoryError>;

    async fn get_library_overview(
        &self,
        tenant_id: TenantId,
    ) -> Result<LibraryOverviewSnapshot, RepositoryError>;

    async fn get_prompt_detail(
        &self,
        tenant_id: TenantId,
        prompt_id: PromptId,
    ) -> Result<Option<PromptDetailSnapshot>, RepositoryError>;

    async fn get_asset_detail(
        &self,
        tenant_id: TenantId,
        asset_id: AssetId,
    ) -> Result<Option<AssetDetailSnapshot>, RepositoryError>;
}
