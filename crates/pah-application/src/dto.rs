use std::collections::BTreeMap;

use pah_domain::{
    Asset, AssetId, AssetLink, AssetLinkId, AssetLocation, AssetRelationType, AssetStatus,
    AssetVisibility, Evaluation, EvaluationId, GenerationCost, GenerationRun, GenerationRunId,
    GenerationSource, GenerationStatus, JsonObject, MediaType, Project, ProjectId, ProjectStatus,
    Prompt, PromptId, PromptStatus, PromptVariant, PromptVariantId, TenantId,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{SearchKind, SearchPage, SearchRecord, SearchRecordId, StagedBlob};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    Agent,
    System,
}

/// Tenant and actor are injected by the trusted adapter, never by an untrusted
/// form payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub tenant_id: TenantId,
    pub actor_type: ActorType,
    pub actor_id: String,
    pub request_id: Option<String>,
}

impl RequestContext {
    #[must_use]
    pub fn local(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            actor_type: ActorType::User,
            actor_id: "local-user".into(),
            request_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptVariantDraft {
    pub provider: String,
    pub model: String,
    pub model_version: Option<String>,
    pub prompt_text: String,
    #[serde(default)]
    pub parameters: JsonObject,
    pub adaptation_notes: Option<String>,
    pub success_tips: Option<String>,
    pub failure_patterns: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatePromptRequest {
    pub project_id: ProjectId,
    pub title: String,
    pub creative_intent: String,
    pub base_prompt: String,
    pub negative_constraints: Option<String>,
    pub initial_variant: Option<PromptVariantDraft>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatePromptVariantRequest {
    pub prompt_id: PromptId,
    pub draft: PromptVariantDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetImportSource {
    Staged {
        blob: StagedBlob,
    },
    External {
        url: String,
        original_filename: Option<String>,
        mime_type: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        duration_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportAssetRequest {
    pub media_type: MediaType,
    pub source: AssetImportSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationCostInput {
    pub amount_micros: u64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenerationOutcome {
    Succeeded,
    Failed { message: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordGenerationRequest {
    pub variant_id: PromptVariantId,
    pub source: GenerationSource,
    pub prompt_text_snapshot: String,
    #[serde(default)]
    pub input_extra: JsonObject,
    #[serde(default)]
    pub parameters_snapshot: JsonObject,
    pub external_url: Option<String>,
    pub cost: Option<GenerationCostInput>,
    #[serde(default)]
    pub output_asset_ids: Vec<AssetId>,
    #[serde(default)]
    pub reference_asset_ids: Vec<AssetId>,
    pub outcome: GenerationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEvaluationRequest {
    pub asset_id: AssetId,
    pub generation_run_id: Option<GenerationRunId>,
    pub overall_score: u8,
    pub is_featured: bool,
    pub is_reusable: bool,
    #[serde(default)]
    pub dimension_scores: BTreeMap<String, u8>,
    pub strengths: Option<String>,
    pub issues: Option<String>,
    pub lesson: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateAssetContextRequest {
    pub asset_id: AssetId,
    pub variant_id: Option<PromptVariantId>,
    pub prompt_text_snapshot: Option<String>,
    #[serde(default)]
    pub parameters_snapshot: JsonObject,
    pub overall_score: Option<u8>,
    #[serde(default)]
    pub is_featured: bool,
    pub lesson: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAssetTagsRequest {
    pub asset_id: AssetId,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SearchLibraryRequest {
    pub query: Option<String>,
    #[serde(default)]
    pub kinds: Vec<SearchKind>,
    pub project_id: Option<ProjectId>,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub media_types: Vec<MediaType>,
    #[serde(default)]
    pub prompt_statuses: Vec<PromptStatus>,
    pub minimum_score: Option<u8>,
    #[serde(default)]
    pub featured_only: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub include_archived: bool,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDto {
    pub id: ProjectId,
    pub name: String,
    pub description: Option<String>,
    pub cover_asset_id: Option<AssetId>,
    pub status: ProjectStatus,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl From<&Project> for ProjectDto {
    fn from(value: &Project) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            description: value.description.clone(),
            cover_asset_id: value.cover_asset_id,
            status: value.status,
            created_at: value.timestamps.created_at,
            updated_at: value.timestamps.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptDto {
    pub id: PromptId,
    pub project_id: ProjectId,
    pub title: String,
    pub creative_intent: String,
    pub base_prompt: String,
    pub negative_constraints: Option<String>,
    pub status: PromptStatus,
    pub current_version: u32,
    pub created_by: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl From<&Prompt> for PromptDto {
    fn from(value: &Prompt) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            title: value.title.clone(),
            creative_intent: value.creative_intent.clone(),
            base_prompt: value.base_prompt.clone(),
            negative_constraints: value.negative_constraints.clone(),
            status: value.status,
            current_version: value.current_version,
            created_by: value.created_by.clone(),
            created_at: value.timestamps.created_at,
            updated_at: value.timestamps.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptVariantDto {
    pub id: PromptVariantId,
    pub prompt_id: PromptId,
    pub provider: String,
    pub model: String,
    pub model_version: Option<String>,
    pub prompt_text: String,
    pub parameters: JsonObject,
    pub adaptation_notes: Option<String>,
    pub success_tips: Option<String>,
    pub failure_patterns: Option<String>,
    pub status: PromptStatus,
    pub revision: u32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl From<&PromptVariant> for PromptVariantDto {
    fn from(value: &PromptVariant) -> Self {
        Self {
            id: value.id,
            prompt_id: value.prompt_id,
            provider: value.provider.clone(),
            model: value.model.clone(),
            model_version: value.model_version.clone(),
            prompt_text: value.prompt_text.clone(),
            parameters: value.parameters.clone(),
            adaptation_notes: value.adaptation_notes.clone(),
            success_tips: value.success_tips.clone(),
            failure_patterns: value.failure_patterns.clone(),
            status: value.status,
            revision: value.revision,
            created_at: value.timestamps.created_at,
            updated_at: value.timestamps.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatedPromptDto {
    pub prompt: PromptDto,
    pub initial_variant: Option<PromptVariantDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetDto {
    pub id: AssetId,
    pub media_type: MediaType,
    pub location: AssetLocation,
    pub original_filename: Option<String>,
    pub mime_type: Option<String>,
    pub byte_size: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub sha256: Option<String>,
    pub thumbnail_key: Option<String>,
    pub status: AssetStatus,
    pub visibility: AssetVisibility,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl From<&Asset> for AssetDto {
    fn from(value: &Asset) -> Self {
        Self {
            id: value.id,
            media_type: value.media_type,
            location: value.location.clone(),
            original_filename: value.original_filename.clone(),
            mime_type: value.mime_type.clone(),
            byte_size: value.byte_size,
            width: value.width,
            height: value.height,
            duration_ms: value.duration_ms,
            sha256: value.sha256.clone(),
            thumbnail_key: value.thumbnail_key.clone(),
            status: value.status,
            visibility: value.visibility,
            created_at: value.timestamps.created_at,
            updated_at: value.timestamps.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRunDto {
    pub id: GenerationRunId,
    pub variant_id: PromptVariantId,
    pub source: GenerationSource,
    pub prompt_text_snapshot: String,
    pub reference_asset_ids: Vec<AssetId>,
    pub input_extra: JsonObject,
    pub parameters_snapshot: JsonObject,
    pub external_url: Option<String>,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub status: GenerationStatus,
    pub failure_message: Option<String>,
    pub cost: Option<GenerationCost>,
    pub created_by: String,
}

impl From<&GenerationRun> for GenerationRunDto {
    fn from(value: &GenerationRun) -> Self {
        Self {
            id: value.id,
            variant_id: value.variant_id,
            source: value.source,
            prompt_text_snapshot: value.input_snapshot.prompt_text.clone(),
            reference_asset_ids: value.input_snapshot.reference_asset_ids.clone(),
            input_extra: value.input_snapshot.extra.clone(),
            parameters_snapshot: value.parameters_snapshot.clone(),
            external_url: value.external_url.clone(),
            started_at: value.started_at,
            completed_at: value.completed_at,
            status: value.status,
            failure_message: value.failure_message.clone(),
            cost: value.cost.clone(),
            created_by: value.created_by.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationDto {
    pub id: EvaluationId,
    pub asset_id: AssetId,
    pub generation_run_id: Option<GenerationRunId>,
    pub overall_score: u8,
    pub is_featured: bool,
    pub is_reusable: bool,
    pub dimension_scores: BTreeMap<String, u8>,
    pub strengths: Option<String>,
    pub issues: Option<String>,
    pub lesson: Option<String>,
    pub created_by: String,
    pub created_at: OffsetDateTime,
}

impl From<&Evaluation> for EvaluationDto {
    fn from(value: &Evaluation) -> Self {
        Self {
            id: value.id,
            asset_id: value.asset_id,
            generation_run_id: value.generation_run_id,
            overall_score: value.overall_score,
            is_featured: value.is_featured,
            is_reusable: value.is_reusable,
            dimension_scores: value.dimension_scores.clone(),
            strengths: value.strengths.clone(),
            issues: value.issues.clone(),
            lesson: value.lesson.clone(),
            created_by: value.created_by.clone(),
            created_at: value.timestamps.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetLinkDto {
    pub id: AssetLinkId,
    pub asset_id: AssetId,
    pub generation_run_id: Option<GenerationRunId>,
    pub prompt_id: Option<PromptId>,
    pub related_asset_id: Option<AssetId>,
    pub relation_type: AssetRelationType,
    pub sort_order: u32,
    pub note: Option<String>,
}

impl From<&AssetLink> for AssetLinkDto {
    fn from(value: &AssetLink) -> Self {
        Self {
            id: value.id,
            asset_id: value.asset_id,
            generation_run_id: value.generation_run_id,
            prompt_id: value.prompt_id,
            related_asset_id: value.related_asset_id,
            relation_type: value.relation_type,
            sort_order: value.sort_order,
            note: value.note.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchItemDto {
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

impl From<SearchRecord> for SearchItemDto {
    fn from(value: SearchRecord) -> Self {
        Self {
            id: value.id,
            title: value.title,
            summary: value.summary,
            project_id: value.project_id,
            project_name: value.project_name,
            media_type: value.media_type,
            provider: value.provider,
            model: value.model,
            thumbnail_key: value.thumbnail_key,
            overall_score: value.overall_score,
            is_featured: value.is_featured,
            tags: value.tags,
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultsDto {
    pub items: Vec<SearchItemDto>,
    pub next_cursor: Option<String>,
}

impl From<SearchPage<SearchRecord>> for SearchResultsDto {
    fn from(value: SearchPage<SearchRecord>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryOverviewDto {
    pub projects: Vec<ProjectDto>,
    pub prompts: Vec<PromptDto>,
    pub variants: Vec<PromptVariantDto>,
    pub variant_result_counts: BTreeMap<PromptVariantId, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptDetailDto {
    pub project: ProjectDto,
    pub prompt: PromptDto,
    pub variants: Vec<PromptVariantDto>,
    pub generation_runs: Vec<GenerationRunDto>,
    pub assets: Vec<AssetDto>,
    pub evaluations: Vec<EvaluationDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetDetailDto {
    pub asset: AssetDto,
    pub links: Vec<AssetLinkDto>,
    pub prompts: Vec<PromptDto>,
    pub variants: Vec<PromptVariantDto>,
    pub generation_runs: Vec<GenerationRunDto>,
    pub evaluations: Vec<EvaluationDto>,
    pub related_assets: Vec<AssetDto>,
    pub tags: Vec<String>,
}
