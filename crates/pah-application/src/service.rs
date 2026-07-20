use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use pah_domain::{
    Asset, AssetFile, AssetFileId, AssetFileRole, AssetId, AssetLink, AssetLinkId,
    AssetRelationType, Evaluation, EvaluationId, GenerationCost, GenerationRun, GenerationRunId,
    InputSnapshot, Project, ProjectId, Prompt, PromptId, PromptVariant, PromptVariantId,
};
use time::OffsetDateTime;

use crate::{
    AppError, AssetContextReplacement, AssetDetailDto, AssetDetailSnapshot, AssetDto,
    AssetImportSource, BlobStore, CreateEvaluationRequest, CreateProjectRequest,
    CreatePromptRequest, CreatePromptVariantRequest, CreatedPromptDto, EvaluationDto,
    GenerationOutcome, GenerationRunDto, ImportAssetRequest, LibraryOverviewDto,
    LibraryOverviewSnapshot, LibraryRepository, ProjectDto, PromptDetailDto, PromptDetailSnapshot,
    PromptDto, PromptVariantDraft, PromptVariantDto, RecordGenerationRequest, RepositoryError,
    RequestContext, SearchCriteria, SearchKind, SearchLibraryRequest, SearchResultsDto, StagedBlob,
    StoreBlobRequest, StoredBlob, UpdateAssetContextRequest, UpdateAssetTagsRequest,
};

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

#[derive(Clone)]
pub struct AppService {
    repository: Arc<dyn LibraryRepository>,
    blob_store: Arc<dyn BlobStore>,
    clock: Arc<dyn Clock>,
}

impl AppService {
    #[must_use]
    pub fn new(repository: Arc<dyn LibraryRepository>, blob_store: Arc<dyn BlobStore>) -> Self {
        Self::with_clock(repository, blob_store, Arc::new(SystemClock))
    }

    #[must_use]
    pub fn with_clock(
        repository: Arc<dyn LibraryRepository>,
        blob_store: Arc<dyn BlobStore>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            repository,
            blob_store,
            clock,
        }
    }

    pub async fn create_project(
        &self,
        context: &RequestContext,
        request: CreateProjectRequest,
    ) -> Result<ProjectDto, AppError> {
        validate_context(context)?;
        let project = Project::new(
            ProjectId::new(),
            context.tenant_id,
            request.name,
            request.description,
            self.clock.now(),
        )?;
        self.repository
            .create_project(context.tenant_id, project.clone())
            .await?;
        Ok(ProjectDto::from(&project))
    }

    pub async fn create_prompt(
        &self,
        context: &RequestContext,
        request: CreatePromptRequest,
    ) -> Result<CreatedPromptDto, AppError> {
        validate_context(context)?;
        if !self
            .repository
            .project_exists(context.tenant_id, request.project_id)
            .await?
        {
            return Err(not_found("project", &request.project_id));
        }

        let now = self.clock.now();
        let prompt = Prompt::new(
            PromptId::new(),
            context.tenant_id,
            request.project_id,
            request.title,
            request.creative_intent,
            request.base_prompt,
            request.negative_constraints,
            context.actor_id.clone(),
            now,
        )?;
        let variant = request
            .initial_variant
            .map(|draft| build_variant(context.tenant_id, prompt.id, draft, now))
            .transpose()?;

        self.repository
            .create_prompt_bundle(context.tenant_id, prompt.clone(), variant.clone())
            .await?;

        Ok(CreatedPromptDto {
            prompt: PromptDto::from(&prompt),
            initial_variant: variant.as_ref().map(PromptVariantDto::from),
        })
    }

    pub async fn create_prompt_variant(
        &self,
        context: &RequestContext,
        request: CreatePromptVariantRequest,
    ) -> Result<PromptVariantDto, AppError> {
        validate_context(context)?;
        if !self
            .repository
            .prompt_exists(context.tenant_id, request.prompt_id)
            .await?
        {
            return Err(not_found("prompt", &request.prompt_id));
        }

        let variant = build_variant(
            context.tenant_id,
            request.prompt_id,
            request.draft,
            self.clock.now(),
        )?;
        self.repository
            .add_prompt_variant(context.tenant_id, variant.clone())
            .await?;
        Ok(PromptVariantDto::from(&variant))
    }

    pub async fn import_asset(
        &self,
        context: &RequestContext,
        request: ImportAssetRequest,
    ) -> Result<AssetDto, AppError> {
        validate_context(context)?;
        let asset_id = AssetId::new();
        let now = self.clock.now();

        let (asset, files) = match request.source {
            AssetImportSource::External {
                url,
                original_filename,
                mime_type,
                width,
                height,
                duration_ms,
            } => (
                Asset::new_external(
                    asset_id,
                    context.tenant_id,
                    request.media_type,
                    url,
                    original_filename,
                    mime_type,
                    width,
                    height,
                    duration_ms,
                    now,
                )?,
                Vec::new(),
            ),
            AssetImportSource::Staged { blob } => {
                self.import_staged_asset(context.tenant_id, asset_id, request.media_type, blob, now)
                    .await?
            }
        };

        if let Err(error) = self
            .repository
            .create_asset(context.tenant_id, asset.clone(), files.clone())
            .await
        {
            for file in files {
                let _ = self
                    .blob_store
                    .delete(context.tenant_id, &file.storage_key)
                    .await;
            }
            return Err(error.into());
        }

        Ok(AssetDto::from(&asset))
    }

    async fn import_staged_asset(
        &self,
        tenant_id: pah_domain::TenantId,
        asset_id: AssetId,
        media_type: pah_domain::MediaType,
        blob: StagedBlob,
        now: OffsetDateTime,
    ) -> Result<(Asset, Vec<AssetFile>), AppError> {
        let original_filename = blob.original_filename.clone();
        let stored = self
            .blob_store
            .store(
                tenant_id,
                StoreBlobRequest {
                    asset_id,
                    staged: blob,
                },
            )
            .await?;
        let cleanup_keys: Vec<String> = std::iter::once(stored.storage_key.clone())
            .chain(
                stored
                    .derivatives
                    .iter()
                    .map(|file| file.storage_key.clone()),
            )
            .collect();
        match build_stored_asset_bundle(
            tenant_id,
            asset_id,
            media_type,
            original_filename,
            stored,
            now,
        ) {
            Ok(bundle) => Ok(bundle),
            Err(error) => {
                for key in cleanup_keys {
                    let _ = self.blob_store.delete(tenant_id, &key).await;
                }
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn record_generation(
        &self,
        context: &RequestContext,
        request: RecordGenerationRequest,
    ) -> Result<GenerationRunDto, AppError> {
        validate_context(context)?;
        let variant = self
            .repository
            .get_prompt_variant(context.tenant_id, request.variant_id)
            .await?
            .ok_or_else(|| not_found("prompt_variant", &request.variant_id))?;
        ensure_tenant(context.tenant_id, variant.tenant_id, "prompt_variant")?;

        let output_ids = unique_ids("output_asset_ids", &request.output_asset_ids)?;
        let reference_ids = unique_ids("reference_asset_ids", &request.reference_asset_ids)?;
        if output_ids.iter().any(|id| reference_ids.contains(id)) {
            return Err(AppError::InvalidRequest(
                "an asset cannot be both an output and a reference in one generation".into(),
            ));
        }
        let all_ids: Vec<_> = output_ids.union(&reference_ids).copied().collect();
        let existing: BTreeSet<_> = self
            .repository
            .existing_asset_ids(context.tenant_id, &all_ids)
            .await?
            .into_iter()
            .collect();
        let missing: Vec<_> = all_ids
            .iter()
            .filter(|id| !existing.contains(id))
            .map(ToString::to_string)
            .collect();
        if !missing.is_empty() {
            return Err(AppError::InvalidRequest(format!(
                "assets are missing from the current tenant: {}",
                missing.join(", ")
            )));
        }

        if matches!(request.outcome, GenerationOutcome::Succeeded)
            && output_ids.is_empty()
            && request
                .external_url
                .as_ref()
                .is_none_or(|url| url.trim().is_empty())
        {
            return Err(AppError::InvalidRequest(
                "a successful generation needs an output asset or external URL".into(),
            ));
        }

        let input_snapshot = InputSnapshot::new(
            request.prompt_text_snapshot,
            reference_ids.iter().copied().collect(),
            request.input_extra,
        )?;
        let cost = request
            .cost
            .map(|cost| GenerationCost::new(cost.amount_micros, cost.currency))
            .transpose()?;
        let now = self.clock.now();
        let mut run = GenerationRun::new(
            GenerationRunId::new(),
            context.tenant_id,
            request.variant_id,
            request.source,
            input_snapshot,
            request.parameters_snapshot,
            request.external_url,
            cost,
            context.actor_id.clone(),
            now,
        )?;
        match request.outcome {
            GenerationOutcome::Succeeded => run.succeed(now)?,
            GenerationOutcome::Failed { message } => run.fail(message, now)?,
            GenerationOutcome::Cancelled => run.cancel(now)?,
        }

        let mut links = Vec::with_capacity(output_ids.len() + reference_ids.len());
        for (sort_order, asset_id) in output_ids.iter().copied().enumerate() {
            let sort_order = u32::try_from(sort_order).map_err(|_| {
                AppError::InvalidRequest("too many output assets in one generation".into())
            })?;
            links.push(AssetLink::new(
                AssetLinkId::new(),
                context.tenant_id,
                asset_id,
                Some(run.id),
                Some(variant.prompt_id),
                None,
                AssetRelationType::Output,
                sort_order,
                None,
                now,
            )?);
        }
        for (sort_order, asset_id) in reference_ids.iter().copied().enumerate() {
            let sort_order = u32::try_from(sort_order).map_err(|_| {
                AppError::InvalidRequest("too many reference assets in one generation".into())
            })?;
            links.push(AssetLink::new(
                AssetLinkId::new(),
                context.tenant_id,
                asset_id,
                Some(run.id),
                Some(variant.prompt_id),
                None,
                AssetRelationType::ReferenceInput,
                sort_order,
                None,
                now,
            )?);
        }

        self.repository
            .record_generation(context.tenant_id, run.clone(), links)
            .await?;
        Ok(GenerationRunDto::from(&run))
    }

    pub async fn create_evaluation(
        &self,
        context: &RequestContext,
        request: CreateEvaluationRequest,
    ) -> Result<EvaluationDto, AppError> {
        validate_context(context)?;
        let existing = self
            .repository
            .existing_asset_ids(context.tenant_id, &[request.asset_id])
            .await?;
        if !existing.contains(&request.asset_id) {
            return Err(not_found("asset", &request.asset_id));
        }

        let evaluation = Evaluation::new(
            EvaluationId::new(),
            context.tenant_id,
            request.asset_id,
            request.generation_run_id,
            request.overall_score,
            request.is_featured,
            request.is_reusable,
            request.dimension_scores,
            request.strengths,
            request.issues,
            request.lesson,
            context.actor_id.clone(),
            self.clock.now(),
        )?;
        self.repository
            .create_evaluation(context.tenant_id, evaluation.clone())
            .await?;
        Ok(EvaluationDto::from(&evaluation))
    }

    pub async fn update_asset_context(
        &self,
        context: &RequestContext,
        request: UpdateAssetContextRequest,
    ) -> Result<AssetDetailDto, AppError> {
        validate_context(context)?;
        let existing = self
            .repository
            .existing_asset_ids(context.tenant_id, &[request.asset_id])
            .await?;
        if !existing.contains(&request.asset_id) {
            return Err(not_found("asset", &request.asset_id));
        }
        if request
            .overall_score
            .is_some_and(|score| !(1..=5).contains(&score))
        {
            return Err(AppError::InvalidRequest(
                "overall_score must be between 1 and 5".into(),
            ));
        }
        if request.overall_score.is_none()
            && (request.is_featured
                || request
                    .lesson
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()))
        {
            return Err(AppError::InvalidRequest(
                "featured and lesson require an overall_score".into(),
            ));
        }

        let variant = if let Some(variant_id) = request.variant_id {
            let variant = self
                .repository
                .get_prompt_variant(context.tenant_id, variant_id)
                .await?
                .ok_or_else(|| not_found("prompt_variant", &variant_id))?;
            ensure_tenant(context.tenant_id, variant.tenant_id, "prompt_variant")?;
            Some(variant)
        } else {
            None
        };
        let asset_id = request.asset_id;
        let replacement =
            build_asset_context_replacement(context, request, variant, self.clock.now())?;
        self.repository
            .replace_asset_context(context.tenant_id, replacement)
            .await?;
        self.get_asset_detail(context, asset_id).await
    }

    pub async fn update_asset_tags(
        &self,
        context: &RequestContext,
        request: UpdateAssetTagsRequest,
    ) -> Result<AssetDetailDto, AppError> {
        validate_context(context)?;
        let existing = self
            .repository
            .existing_asset_ids(context.tenant_id, &[request.asset_id])
            .await?;
        if !existing.contains(&request.asset_id) {
            return Err(not_found("asset", &request.asset_id));
        }
        let tags = normalize_asset_tags(request.tags)?;
        self.repository
            .replace_asset_tags(context.tenant_id, request.asset_id, tags, self.clock.now())
            .await?;
        self.get_asset_detail(context, request.asset_id).await
    }

    pub async fn search_library(
        &self,
        context: &RequestContext,
        request: SearchLibraryRequest,
    ) -> Result<SearchResultsDto, AppError> {
        validate_context(context)?;
        if request
            .minimum_score
            .is_some_and(|score| !(1..=5).contains(&score))
        {
            return Err(AppError::InvalidRequest(
                "minimum_score must be between 1 and 5".into(),
            ));
        }
        let criteria = SearchCriteria {
            query: clean_optional(request.query),
            kinds: if request.kinds.is_empty() {
                vec![SearchKind::Prompt, SearchKind::Asset]
            } else {
                request.kinds
            },
            project_id: request.project_id,
            providers: clean_list(request.providers),
            models: clean_list(request.models),
            media_types: request.media_types,
            prompt_statuses: request.prompt_statuses,
            minimum_score: request.minimum_score,
            featured_only: request.featured_only,
            tags: clean_list(request.tags),
            include_archived: request.include_archived,
            limit: request.limit.unwrap_or(30).clamp(1, 100),
            cursor: clean_optional(request.cursor),
        };
        let page = self
            .repository
            .search_library(context.tenant_id, criteria)
            .await?;
        if page
            .items
            .iter()
            .any(|item| item.tenant_id != context.tenant_id)
        {
            return Err(RepositoryError::CorruptData(
                "search returned a row from another tenant".into(),
            )
            .into());
        }
        Ok(page.into())
    }

    pub async fn get_library_overview(
        &self,
        context: &RequestContext,
    ) -> Result<LibraryOverviewDto, AppError> {
        validate_context(context)?;
        let snapshot = self
            .repository
            .get_library_overview(context.tenant_id)
            .await?;
        validate_overview_snapshot(context.tenant_id, &snapshot)?;
        Ok(LibraryOverviewDto {
            projects: snapshot.projects.iter().map(ProjectDto::from).collect(),
            prompts: snapshot.prompts.iter().map(PromptDto::from).collect(),
            variants: snapshot
                .variants
                .iter()
                .map(PromptVariantDto::from)
                .collect(),
            variant_result_counts: snapshot.variant_result_counts.clone(),
        })
    }

    pub async fn get_prompt_detail(
        &self,
        context: &RequestContext,
        prompt_id: PromptId,
    ) -> Result<PromptDetailDto, AppError> {
        validate_context(context)?;
        let snapshot = self
            .repository
            .get_prompt_detail(context.tenant_id, prompt_id)
            .await?
            .ok_or_else(|| not_found("prompt", &prompt_id))?;
        validate_prompt_snapshot(context.tenant_id, &snapshot)?;
        Ok(prompt_snapshot_to_dto(&snapshot))
    }

    pub async fn get_asset_detail(
        &self,
        context: &RequestContext,
        asset_id: AssetId,
    ) -> Result<AssetDetailDto, AppError> {
        validate_context(context)?;
        let snapshot = self
            .repository
            .get_asset_detail(context.tenant_id, asset_id)
            .await?
            .ok_or_else(|| not_found("asset", &asset_id))?;
        validate_asset_snapshot(context.tenant_id, &snapshot)?;
        Ok(asset_snapshot_to_dto(&snapshot))
    }
}

fn build_stored_asset_bundle(
    tenant_id: pah_domain::TenantId,
    asset_id: AssetId,
    media_type: pah_domain::MediaType,
    original_filename: Option<String>,
    stored: StoredBlob,
    now: OffsetDateTime,
) -> Result<(Asset, Vec<AssetFile>), AppError> {
    let mut asset = Asset::new_stored(
        asset_id,
        tenant_id,
        media_type,
        stored.storage_key.clone(),
        original_filename,
        stored.detected_mime_type.clone(),
        stored.byte_size,
        stored.width,
        stored.height,
        stored.duration_ms,
        stored.sha256.clone(),
        now,
    )?;
    if let Some(thumbnail) = stored
        .derivatives
        .iter()
        .find(|file| file.role == AssetFileRole::Thumbnail)
    {
        asset.set_thumbnail_key(thumbnail.storage_key.clone())?;
    }
    let mut files = Vec::with_capacity(1 + stored.derivatives.len());
    files.push(AssetFile::new(
        AssetFileId::new(),
        tenant_id,
        asset_id,
        AssetFileRole::Original,
        stored.storage_key,
        stored.detected_mime_type,
        stored.byte_size,
        stored.width,
        stored.height,
        stored.sha256,
        None,
        now,
    )?);
    for derivative in stored.derivatives {
        files.push(AssetFile::new(
            AssetFileId::new(),
            tenant_id,
            asset_id,
            derivative.role,
            derivative.storage_key,
            derivative.detected_mime_type,
            derivative.byte_size,
            derivative.width,
            derivative.height,
            derivative.sha256,
            derivative.generator_version,
            now,
        )?);
    }
    Ok((asset, files))
}

fn build_variant(
    tenant_id: pah_domain::TenantId,
    prompt_id: PromptId,
    draft: PromptVariantDraft,
    now: OffsetDateTime,
) -> Result<PromptVariant, pah_domain::DomainError> {
    PromptVariant::new(
        PromptVariantId::new(),
        tenant_id,
        prompt_id,
        draft.provider,
        draft.model,
        draft.model_version,
        draft.prompt_text,
        draft.parameters,
        draft.adaptation_notes,
        draft.success_tips,
        draft.failure_patterns,
        now,
    )
}

fn validate_context(context: &RequestContext) -> Result<(), AppError> {
    if context.actor_id.trim().is_empty() {
        return Err(AppError::InvalidRequest(
            "actor_id must not be empty".into(),
        ));
    }
    if context
        .request_id
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AppError::InvalidRequest(
            "request_id must not be blank when supplied".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn build_asset_context_replacement(
    context: &RequestContext,
    request: UpdateAssetContextRequest,
    variant: Option<PromptVariant>,
    now: OffsetDateTime,
) -> Result<AssetContextReplacement, AppError> {
    let (generation_run, asset_link) = if let Some(variant) = variant {
        let prompt_text = request
            .prompt_text_snapshot
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| variant.prompt_text.clone());
        let input_snapshot = InputSnapshot::new(prompt_text, Vec::new(), BTreeMap::default())?;
        let mut run = GenerationRun::new(
            GenerationRunId::new(),
            context.tenant_id,
            variant.id,
            pah_domain::GenerationSource::Manual,
            input_snapshot,
            request.parameters_snapshot,
            None,
            None,
            context.actor_id.clone(),
            now,
        )?;
        run.succeed(now)?;
        let link = AssetLink::new(
            AssetLinkId::new(),
            context.tenant_id,
            request.asset_id,
            Some(run.id),
            Some(variant.prompt_id),
            None,
            AssetRelationType::Output,
            0,
            Some("manual metadata revision".into()),
            now,
        )?;
        (Some(run), Some(link))
    } else {
        (None, None)
    };
    let evaluation = request
        .overall_score
        .map(|score| {
            Evaluation::new(
                EvaluationId::new(),
                context.tenant_id,
                request.asset_id,
                generation_run.as_ref().map(|run| run.id),
                score,
                request.is_featured,
                score >= 4,
                BTreeMap::default(),
                None,
                None,
                request.lesson,
                context.actor_id.clone(),
                now,
            )
        })
        .transpose()?;
    Ok(AssetContextReplacement {
        asset_id: request.asset_id,
        generation_run,
        asset_link,
        evaluation,
        replaced_at: now,
    })
}

fn not_found(entity: &'static str, id: &impl ToString) -> AppError {
    AppError::NotFound {
        entity,
        id: id.to_string(),
    }
}

fn ensure_tenant(
    expected: pah_domain::TenantId,
    actual: pah_domain::TenantId,
    entity: &'static str,
) -> Result<(), AppError> {
    if expected == actual {
        Ok(())
    } else {
        Err(RepositoryError::CorruptData(format!("{entity} escaped tenant scope")).into())
    }
}

fn unique_ids(field: &str, values: &[AssetId]) -> Result<BTreeSet<AssetId>, AppError> {
    let set: BTreeSet<_> = values.iter().copied().collect();
    if set.len() == values.len() {
        Ok(set)
    } else {
        Err(AppError::InvalidRequest(format!(
            "{field} contains duplicate asset IDs"
        )))
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn clean_list(values: Vec<String>) -> Vec<String> {
    let mut values: Vec<_> = values
        .into_iter()
        .filter_map(|value| clean_optional(Some(value)))
        .collect();
    values.sort();
    values.dedup();
    values
}

fn normalize_asset_tags(values: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut seen = BTreeSet::new();
    let mut tags = Vec::new();
    for value in values {
        let tag = value
            .trim()
            .trim_start_matches(['#', '＃'])
            .trim()
            .to_owned();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > 32 {
            return Err(AppError::InvalidRequest(
                "each tag must contain at most 32 characters".into(),
            ));
        }
        if tag.chars().any(char::is_control) {
            return Err(AppError::InvalidRequest(
                "tags cannot contain control characters".into(),
            ));
        }
        if seen.insert(tag.to_lowercase()) {
            tags.push(tag);
        }
    }
    if tags.len() > 20 {
        return Err(AppError::InvalidRequest(
            "an asset can contain at most 20 tags".into(),
        ));
    }
    Ok(tags)
}

fn validate_prompt_snapshot(
    tenant_id: pah_domain::TenantId,
    snapshot: &PromptDetailSnapshot,
) -> Result<(), AppError> {
    ensure_tenant(tenant_id, snapshot.project.tenant_id, "project")?;
    ensure_tenant(tenant_id, snapshot.prompt.tenant_id, "prompt")?;
    for variant in &snapshot.variants {
        ensure_tenant(tenant_id, variant.tenant_id, "prompt_variant")?;
    }
    for run in &snapshot.generation_runs {
        ensure_tenant(tenant_id, run.tenant_id, "generation_run")?;
    }
    for asset in &snapshot.assets {
        ensure_tenant(tenant_id, asset.tenant_id, "asset")?;
    }
    for evaluation in &snapshot.evaluations {
        ensure_tenant(tenant_id, evaluation.tenant_id, "evaluation")?;
    }
    Ok(())
}

fn validate_overview_snapshot(
    tenant_id: pah_domain::TenantId,
    snapshot: &LibraryOverviewSnapshot,
) -> Result<(), AppError> {
    for project in &snapshot.projects {
        ensure_tenant(tenant_id, project.tenant_id, "project")?;
    }
    for prompt in &snapshot.prompts {
        ensure_tenant(tenant_id, prompt.tenant_id, "prompt")?;
    }
    for variant in &snapshot.variants {
        ensure_tenant(tenant_id, variant.tenant_id, "prompt_variant")?;
    }
    Ok(())
}

fn validate_asset_snapshot(
    tenant_id: pah_domain::TenantId,
    snapshot: &AssetDetailSnapshot,
) -> Result<(), AppError> {
    ensure_tenant(tenant_id, snapshot.asset.tenant_id, "asset")?;
    for link in &snapshot.links {
        ensure_tenant(tenant_id, link.tenant_id, "asset_link")?;
    }
    for prompt in &snapshot.prompts {
        ensure_tenant(tenant_id, prompt.tenant_id, "prompt")?;
    }
    for variant in &snapshot.variants {
        ensure_tenant(tenant_id, variant.tenant_id, "prompt_variant")?;
    }
    for run in &snapshot.generation_runs {
        ensure_tenant(tenant_id, run.tenant_id, "generation_run")?;
    }
    for evaluation in &snapshot.evaluations {
        ensure_tenant(tenant_id, evaluation.tenant_id, "evaluation")?;
    }
    for asset in &snapshot.related_assets {
        ensure_tenant(tenant_id, asset.tenant_id, "related_asset")?;
    }
    Ok(())
}

fn prompt_snapshot_to_dto(snapshot: &PromptDetailSnapshot) -> PromptDetailDto {
    PromptDetailDto {
        project: ProjectDto::from(&snapshot.project),
        prompt: PromptDto::from(&snapshot.prompt),
        variants: snapshot
            .variants
            .iter()
            .map(PromptVariantDto::from)
            .collect(),
        generation_runs: snapshot
            .generation_runs
            .iter()
            .map(GenerationRunDto::from)
            .collect(),
        assets: snapshot.assets.iter().map(AssetDto::from).collect(),
        evaluations: snapshot
            .evaluations
            .iter()
            .map(EvaluationDto::from)
            .collect(),
    }
}

fn asset_snapshot_to_dto(snapshot: &AssetDetailSnapshot) -> AssetDetailDto {
    AssetDetailDto {
        asset: AssetDto::from(&snapshot.asset),
        links: snapshot.links.iter().map(Into::into).collect(),
        prompts: snapshot.prompts.iter().map(PromptDto::from).collect(),
        variants: snapshot
            .variants
            .iter()
            .map(PromptVariantDto::from)
            .collect(),
        generation_runs: snapshot
            .generation_runs
            .iter()
            .map(GenerationRunDto::from)
            .collect(),
        evaluations: snapshot
            .evaluations
            .iter()
            .map(EvaluationDto::from)
            .collect(),
        related_assets: snapshot.related_assets.iter().map(AssetDto::from).collect(),
        tags: snapshot.tags.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use pah_domain::{
        Asset, AssetId, AssetLink, Evaluation, GenerationRun, MediaType, Project, ProjectId,
        Prompt, PromptId, PromptVariant, PromptVariantId, TenantId,
    };
    use time::OffsetDateTime;

    use super::{AppService, Clock, normalize_asset_tags};
    use crate::{
        AssetDetailSnapshot, BlobStore, BlobStoreError, CreateProjectRequest,
        LibraryOverviewSnapshot, LibraryRepository, PromptDetailSnapshot, RepositoryError,
        RequestContext, SearchCriteria, SearchKind, SearchLibraryRequest, SearchPage, SearchRecord,
        SearchRecordId, StoreBlobRequest, StoredBlob,
    };

    #[derive(Default)]
    struct RecordingRepository {
        projects: Mutex<Vec<Project>>,
        searches: Mutex<Vec<SearchCriteria>>,
        search_result: Mutex<Option<SearchPage<SearchRecord>>>,
    }

    impl RecordingRepository {
        fn unsupported() -> RepositoryError {
            RepositoryError::Unsupported("not used by this test".into())
        }
    }

    #[async_trait]
    impl LibraryRepository for RecordingRepository {
        async fn create_project(
            &self,
            _tenant_id: TenantId,
            project: Project,
        ) -> Result<(), RepositoryError> {
            self.projects.lock().expect("projects lock").push(project);
            Ok(())
        }

        async fn project_exists(
            &self,
            _tenant_id: TenantId,
            _project_id: ProjectId,
        ) -> Result<bool, RepositoryError> {
            Err(Self::unsupported())
        }

        async fn create_prompt_bundle(
            &self,
            _tenant_id: TenantId,
            _prompt: Prompt,
            _initial_variant: Option<PromptVariant>,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn prompt_exists(
            &self,
            _tenant_id: TenantId,
            _prompt_id: PromptId,
        ) -> Result<bool, RepositoryError> {
            Err(Self::unsupported())
        }

        async fn add_prompt_variant(
            &self,
            _tenant_id: TenantId,
            _variant: PromptVariant,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn get_prompt_variant(
            &self,
            _tenant_id: TenantId,
            _variant_id: PromptVariantId,
        ) -> Result<Option<PromptVariant>, RepositoryError> {
            Err(Self::unsupported())
        }

        async fn create_asset(
            &self,
            _tenant_id: TenantId,
            _asset: Asset,
            _files: Vec<pah_domain::AssetFile>,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn existing_asset_ids(
            &self,
            _tenant_id: TenantId,
            _asset_ids: &[AssetId],
        ) -> Result<Vec<AssetId>, RepositoryError> {
            Err(Self::unsupported())
        }

        async fn record_generation(
            &self,
            _tenant_id: TenantId,
            _generation_run: GenerationRun,
            _asset_links: Vec<AssetLink>,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn create_evaluation(
            &self,
            _tenant_id: TenantId,
            _evaluation: Evaluation,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn replace_asset_tags(
            &self,
            _tenant_id: TenantId,
            _asset_id: AssetId,
            _tags: Vec<String>,
            _updated_at: OffsetDateTime,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn replace_asset_context(
            &self,
            _tenant_id: TenantId,
            _replacement: crate::AssetContextReplacement,
        ) -> Result<(), RepositoryError> {
            Err(Self::unsupported())
        }

        async fn search_library(
            &self,
            _tenant_id: TenantId,
            criteria: SearchCriteria,
        ) -> Result<SearchPage<SearchRecord>, RepositoryError> {
            self.searches.lock().expect("searches lock").push(criteria);
            Ok(self
                .search_result
                .lock()
                .expect("search result lock")
                .take()
                .unwrap_or(SearchPage {
                    items: Vec::new(),
                    next_cursor: None,
                }))
        }

        async fn get_library_overview(
            &self,
            _tenant_id: TenantId,
        ) -> Result<LibraryOverviewSnapshot, RepositoryError> {
            Err(Self::unsupported())
        }

        async fn get_prompt_detail(
            &self,
            _tenant_id: TenantId,
            _prompt_id: PromptId,
        ) -> Result<Option<PromptDetailSnapshot>, RepositoryError> {
            Err(Self::unsupported())
        }

        async fn get_asset_detail(
            &self,
            _tenant_id: TenantId,
            _asset_id: AssetId,
        ) -> Result<Option<AssetDetailSnapshot>, RepositoryError> {
            Err(Self::unsupported())
        }
    }

    struct RejectingBlobStore;

    #[async_trait]
    impl BlobStore for RejectingBlobStore {
        async fn store(
            &self,
            _tenant_id: TenantId,
            _request: StoreBlobRequest,
        ) -> Result<StoredBlob, BlobStoreError> {
            Err(BlobStoreError::Other("not used by this test".into()))
        }

        async fn delete(
            &self,
            _tenant_id: TenantId,
            _storage_key: &str,
        ) -> Result<(), BlobStoreError> {
            Err(BlobStoreError::Other("not used by this test".into()))
        }
    }

    struct FixedClock(OffsetDateTime);

    impl Clock for FixedClock {
        fn now(&self) -> OffsetDateTime {
            self.0
        }
    }

    fn service(repository: Arc<RecordingRepository>) -> AppService {
        AppService::with_clock(
            repository,
            Arc::new(RejectingBlobStore),
            Arc::new(FixedClock(OffsetDateTime::UNIX_EPOCH)),
        )
    }

    #[tokio::test]
    async fn creates_a_project_in_the_trusted_tenant_context() {
        let repository = Arc::new(RecordingRepository::default());
        let service = service(Arc::clone(&repository));
        let tenant_id = TenantId::new();
        let context = RequestContext::local(tenant_id);

        let created = service
            .create_project(
                &context,
                CreateProjectRequest {
                    name: "  Product launch  ".into(),
                    description: Some("  Summer campaign  ".into()),
                },
            )
            .await
            .expect("create project");

        assert_eq!(created.name, "Product launch");
        let projects = repository.projects.lock().expect("projects lock");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].tenant_id, tenant_id);
        assert_eq!(
            projects[0].timestamps.created_at,
            OffsetDateTime::UNIX_EPOCH
        );
    }

    #[test]
    fn normalizes_asset_tags_without_losing_display_case() {
        let tags = normalize_asset_tags(vec![
            " #雨夜 ".into(),
            "Portrait".into(),
            "portrait".into(),
            "＃暖光".into(),
        ])
        .expect("valid tags");
        assert_eq!(tags, vec!["雨夜", "Portrait", "暖光"]);
        assert!(normalize_asset_tags(vec!["x".repeat(33)]).is_err());
    }

    #[tokio::test]
    async fn normalizes_search_inputs_and_rejects_cross_tenant_rows() {
        let repository = Arc::new(RecordingRepository::default());
        let tenant_id = TenantId::new();
        let other_tenant = TenantId::new();
        *repository.search_result.lock().expect("search result lock") = Some(SearchPage {
            items: vec![SearchRecord {
                tenant_id: other_tenant,
                id: SearchRecordId::Asset(AssetId::new()),
                title: "escaped asset".into(),
                summary: None,
                project_id: None,
                project_name: None,
                media_type: Some(MediaType::Image),
                provider: None,
                model: None,
                thumbnail_key: None,
                overall_score: None,
                is_featured: false,
                tags: Vec::new(),
                created_at: OffsetDateTime::UNIX_EPOCH,
            }],
            next_cursor: None,
        });
        let service = service(Arc::clone(&repository));

        let error = service
            .search_library(
                &RequestContext::local(tenant_id),
                SearchLibraryRequest {
                    query: Some("  skincare  ".into()),
                    kinds: vec![SearchKind::Asset],
                    providers: vec![" openai ".into(), "openai".into()],
                    limit: Some(999),
                    ..SearchLibraryRequest::default()
                },
            )
            .await
            .expect_err("cross-tenant search row rejected");

        assert!(error.to_string().contains("another tenant"));
        let searches = repository.searches.lock().expect("searches lock");
        assert_eq!(searches[0].query.as_deref(), Some("skincare"));
        assert_eq!(searches[0].providers, vec!["openai"]);
        assert_eq!(searches[0].limit, 100);
    }
}
