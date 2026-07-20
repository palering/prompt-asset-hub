use std::collections::{BTreeMap, HashSet};

use async_trait::async_trait;
use pah_application::{
    AssetContextReplacement, AssetDetailSnapshot, LibraryOverviewSnapshot, LibraryRepository,
    PromptDetailSnapshot, RepositoryError, SearchCriteria, SearchKind, SearchPage, SearchRecord,
    SearchRecordId,
};
use pah_domain::{
    Asset, AssetFile, AssetId, AssetLink, AssetLocation, Evaluation, GenerationCost, GenerationRun,
    Project, ProjectId, Prompt, PromptId, PromptVariant, PromptVariantId, RecordTimestamps, TagId,
    TenantId,
};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};

use crate::{
    SqliteLibraryRepository, SqliteRepositoryError, decode_enum, encode_enum, ensure_tenant,
    format_optional_timestamp, format_timestamp, parse_id, parse_optional_timestamp,
    parse_timestamp,
};

#[async_trait]
impl LibraryRepository for SqliteLibraryRepository {
    async fn create_project(
        &self,
        tenant_id: TenantId,
        project: Project,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, project.tenant_id).map_err(repository_error)?;
        let mut connection = self.pool.acquire().await.map_err(database_error)?;
        insert_project(&mut connection, &project)
            .await
            .map_err(repository_error)
    }

    async fn project_exists(
        &self,
        tenant_id: TenantId,
        project_id: ProjectId,
    ) -> Result<bool, RepositoryError> {
        exists(&self.pool, "projects", tenant_id, project_id.to_string()).await
    }

    async fn create_prompt_bundle(
        &self,
        tenant_id: TenantId,
        prompt: Prompt,
        initial_variant: Option<PromptVariant>,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, prompt.tenant_id).map_err(repository_error)?;
        if let Some(variant) = &initial_variant {
            ensure_tenant(tenant_id, variant.tenant_id).map_err(repository_error)?;
            if variant.prompt_id != prompt.id {
                return Err(RepositoryError::Other(
                    "initial variant belongs to a different prompt".into(),
                ));
            }
        }

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        insert_prompt(&mut transaction, &prompt)
            .await
            .map_err(repository_error)?;
        if let Some(variant) = initial_variant {
            insert_prompt_variant(&mut transaction, &variant)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn prompt_exists(
        &self,
        tenant_id: TenantId,
        prompt_id: PromptId,
    ) -> Result<bool, RepositoryError> {
        exists(&self.pool, "prompts", tenant_id, prompt_id.to_string()).await
    }

    async fn add_prompt_variant(
        &self,
        tenant_id: TenantId,
        variant: PromptVariant,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, variant.tenant_id).map_err(repository_error)?;
        let mut connection = self.pool.acquire().await.map_err(database_error)?;
        insert_prompt_variant(&mut connection, &variant)
            .await
            .map_err(repository_error)
    }

    async fn get_prompt_variant(
        &self,
        tenant_id: TenantId,
        variant_id: PromptVariantId,
    ) -> Result<Option<PromptVariant>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, prompt_id, provider, model, model_version,
                   prompt_text, parameters_json, adaptation_notes, success_tips,
                   failure_patterns, status, revision, created_at, updated_at, deleted_at
            FROM prompt_variants
            WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(tenant_id.to_string())
        .bind(variant_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;

        row.map(|row| prompt_variant_from_row(&row).map_err(repository_error))
            .transpose()
    }

    async fn create_asset(
        &self,
        tenant_id: TenantId,
        asset: Asset,
        files: Vec<AssetFile>,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, asset.tenant_id).map_err(repository_error)?;
        for file in &files {
            ensure_tenant(tenant_id, file.tenant_id).map_err(repository_error)?;
            if file.asset_id != asset.id {
                return Err(RepositoryError::Other(
                    "physical file belongs to a different asset".into(),
                ));
            }
        }
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        insert_asset(&mut transaction, &asset)
            .await
            .map_err(repository_error)?;
        for file in &files {
            insert_asset_file(&mut transaction, file)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn existing_asset_ids(
        &self,
        tenant_id: TenantId,
        asset_ids: &[AssetId],
    ) -> Result<Vec<AssetId>, RepositoryError> {
        if asset_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut query = QueryBuilder::<Sqlite>::new("SELECT id FROM assets WHERE tenant_id = ");
        query
            .push_bind(tenant_id.to_string())
            .push(" AND deleted_at IS NULL AND id IN (");
        let mut separated = query.separated(", ");
        for asset_id in asset_ids {
            separated.push_bind(asset_id.to_string());
        }
        separated.push_unseparated(")");

        let rows = query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        let found = rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("id"))
            .collect::<Result<HashSet<_>, _>>()
            .map_err(database_error)?;

        Ok(asset_ids
            .iter()
            .copied()
            .filter(|asset_id| found.contains(&asset_id.to_string()))
            .collect())
    }

    async fn record_generation(
        &self,
        tenant_id: TenantId,
        generation_run: GenerationRun,
        asset_links: Vec<AssetLink>,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, generation_run.tenant_id).map_err(repository_error)?;
        for link in &asset_links {
            ensure_tenant(tenant_id, link.tenant_id).map_err(repository_error)?;
            if link.generation_run_id != Some(generation_run.id) {
                return Err(RepositoryError::Other(format!(
                    "asset link {} does not belong to generation run {}",
                    link.id, generation_run.id
                )));
            }
        }

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        insert_generation_run(&mut transaction, &generation_run)
            .await
            .map_err(repository_error)?;
        for link in &asset_links {
            insert_asset_link(&mut transaction, link)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn create_evaluation(
        &self,
        tenant_id: TenantId,
        evaluation: Evaluation,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, evaluation.tenant_id).map_err(repository_error)?;
        let mut connection = self.pool.acquire().await.map_err(database_error)?;
        insert_evaluation(&mut connection, &evaluation)
            .await
            .map_err(repository_error)
    }

    async fn replace_asset_tags(
        &self,
        tenant_id: TenantId,
        asset_id: AssetId,
        tags: Vec<String>,
        updated_at: time::OffsetDateTime,
    ) -> Result<(), RepositoryError> {
        let timestamp = format_timestamp(updated_at).map_err(repository_error)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let asset_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM assets \
             WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL)",
        )
        .bind(tenant_id.to_string())
        .bind(asset_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        if !asset_exists {
            return Err(RepositoryError::Other(
                "cannot tag an asset outside the requested tenant".into(),
            ));
        }
        sqlx::query("DELETE FROM asset_tags WHERE tenant_id = ? AND asset_id = ?")
            .bind(tenant_id.to_string())
            .bind(asset_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        for tag in tags {
            let existing_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM tags WHERE tenant_id = ? AND category = 'custom' \
                 AND name = ? COLLATE NOCASE AND deleted_at IS NULL LIMIT 1",
            )
            .bind(tenant_id.to_string())
            .bind(&tag)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error)?;
            let tag_id = existing_id.unwrap_or_else(|| TagId::new().to_string());
            sqlx::query(
                "INSERT OR IGNORE INTO tags \
                 (id, tenant_id, name, category, parent_id, created_at, updated_at, deleted_at) \
                 VALUES (?, ?, ?, 'custom', NULL, ?, ?, NULL)",
            )
            .bind(&tag_id)
            .bind(tenant_id.to_string())
            .bind(&tag)
            .bind(&timestamp)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
            sqlx::query(
                "INSERT INTO asset_tags (tenant_id, asset_id, tag_id, created_at) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(tenant_id.to_string())
            .bind(asset_id.to_string())
            .bind(tag_id)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn replace_asset_context(
        &self,
        tenant_id: TenantId,
        replacement: AssetContextReplacement,
    ) -> Result<(), RepositoryError> {
        if let Some(run) = &replacement.generation_run {
            ensure_tenant(tenant_id, run.tenant_id).map_err(repository_error)?;
        }
        if let Some(link) = &replacement.asset_link {
            ensure_tenant(tenant_id, link.tenant_id).map_err(repository_error)?;
            if link.asset_id != replacement.asset_id
                || link.generation_run_id != replacement.generation_run.as_ref().map(|run| run.id)
            {
                return Err(RepositoryError::Other(
                    "replacement link does not match the asset generation context".into(),
                ));
            }
        }
        if replacement.generation_run.is_some() != replacement.asset_link.is_some() {
            return Err(RepositoryError::Other(
                "generation run and asset link must be supplied together".into(),
            ));
        }
        if let Some(evaluation) = &replacement.evaluation {
            ensure_tenant(tenant_id, evaluation.tenant_id).map_err(repository_error)?;
            if evaluation.asset_id != replacement.asset_id {
                return Err(RepositoryError::Other(
                    "replacement evaluation belongs to a different asset".into(),
                ));
            }
        }

        let timestamp = format_timestamp(replacement.replaced_at).map_err(repository_error)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "UPDATE asset_links SET deleted_at = ?, updated_at = ? \
             WHERE tenant_id = ? AND asset_id = ? AND generation_run_id IS NOT NULL \
               AND deleted_at IS NULL",
        )
        .bind(&timestamp)
        .bind(&timestamp)
        .bind(tenant_id.to_string())
        .bind(replacement.asset_id.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        sqlx::query(
            "UPDATE evaluations SET deleted_at = ?, updated_at = ? \
             WHERE tenant_id = ? AND asset_id = ? AND deleted_at IS NULL",
        )
        .bind(&timestamp)
        .bind(&timestamp)
        .bind(tenant_id.to_string())
        .bind(replacement.asset_id.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if let Some(run) = &replacement.generation_run {
            insert_generation_run(&mut transaction, run)
                .await
                .map_err(repository_error)?;
        }
        if let Some(link) = &replacement.asset_link {
            insert_asset_link(&mut transaction, link)
                .await
                .map_err(repository_error)?;
        }
        if let Some(evaluation) = &replacement.evaluation {
            insert_evaluation(&mut transaction, evaluation)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn search_library(
        &self,
        tenant_id: TenantId,
        criteria: SearchCriteria,
    ) -> Result<SearchPage<SearchRecord>, RepositoryError> {
        search_library(self, tenant_id, &criteria).await
    }

    async fn get_library_overview(
        &self,
        tenant_id: TenantId,
    ) -> Result<LibraryOverviewSnapshot, RepositoryError> {
        get_library_overview(self, tenant_id).await
    }

    async fn get_prompt_detail(
        &self,
        tenant_id: TenantId,
        prompt_id: PromptId,
    ) -> Result<Option<PromptDetailSnapshot>, RepositoryError> {
        get_prompt_detail(self, tenant_id, prompt_id).await
    }

    async fn get_asset_detail(
        &self,
        tenant_id: TenantId,
        asset_id: AssetId,
    ) -> Result<Option<AssetDetailSnapshot>, RepositoryError> {
        get_asset_detail(self, tenant_id, asset_id).await
    }
}

async fn exists<I: ToString>(
    pool: &sqlx::SqlitePool,
    table: &'static str,
    tenant_id: TenantId,
    id: I,
) -> Result<bool, RepositoryError> {
    let sql = match table {
        "projects" => {
            "SELECT EXISTS(SELECT 1 FROM projects WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL)"
        }
        "prompts" => {
            "SELECT EXISTS(SELECT 1 FROM prompts WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL)"
        }
        _ => return Err(RepositoryError::Other("unsupported existence query".into())),
    };
    sqlx::query_scalar::<_, bool>(sql)
        .bind(tenant_id.to_string())
        .bind(id.to_string())
        .fetch_one(pool)
        .await
        .map_err(database_error)
}

async fn insert_project(
    connection: &mut SqliteConnection,
    project: &Project,
) -> Result<(), SqliteRepositoryError> {
    sqlx::query(
        r#"
        INSERT INTO projects (
            id, tenant_id, name, description, cover_asset_id, status,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(project.id.to_string())
    .bind(project.tenant_id.to_string())
    .bind(&project.name)
    .bind(&project.description)
    .bind(project.cover_asset_id.map(|id| id.to_string()))
    .bind(encode_enum(&project.status)?)
    .bind(format_timestamp(project.timestamps.created_at)?)
    .bind(format_timestamp(project.timestamps.updated_at)?)
    .bind(format_optional_timestamp(project.timestamps.deleted_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_prompt(
    connection: &mut SqliteConnection,
    prompt: &Prompt,
) -> Result<(), SqliteRepositoryError> {
    sqlx::query(
        r#"
        INSERT INTO prompts (
            id, tenant_id, project_id, title, creative_intent, base_prompt,
            negative_constraints, status, current_version, created_by,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(prompt.id.to_string())
    .bind(prompt.tenant_id.to_string())
    .bind(prompt.project_id.to_string())
    .bind(&prompt.title)
    .bind(&prompt.creative_intent)
    .bind(&prompt.base_prompt)
    .bind(&prompt.negative_constraints)
    .bind(encode_enum(&prompt.status)?)
    .bind(i64::from(prompt.current_version))
    .bind(&prompt.created_by)
    .bind(format_timestamp(prompt.timestamps.created_at)?)
    .bind(format_timestamp(prompt.timestamps.updated_at)?)
    .bind(format_optional_timestamp(prompt.timestamps.deleted_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_prompt_variant(
    connection: &mut SqliteConnection,
    variant: &PromptVariant,
) -> Result<(), SqliteRepositoryError> {
    sqlx::query(
        r#"
        INSERT INTO prompt_variants (
            id, tenant_id, prompt_id, provider, model, model_version,
            prompt_text, parameters_json, adaptation_notes, success_tips,
            failure_patterns, status, revision, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(variant.id.to_string())
    .bind(variant.tenant_id.to_string())
    .bind(variant.prompt_id.to_string())
    .bind(&variant.provider)
    .bind(&variant.model)
    .bind(&variant.model_version)
    .bind(&variant.prompt_text)
    .bind(serde_json::to_string(&variant.parameters)?)
    .bind(&variant.adaptation_notes)
    .bind(&variant.success_tips)
    .bind(&variant.failure_patterns)
    .bind(encode_enum(&variant.status)?)
    .bind(i64::from(variant.revision))
    .bind(format_timestamp(variant.timestamps.created_at)?)
    .bind(format_timestamp(variant.timestamps.updated_at)?)
    .bind(format_optional_timestamp(variant.timestamps.deleted_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_asset(
    connection: &mut SqliteConnection,
    asset: &Asset,
) -> Result<(), SqliteRepositoryError> {
    let (storage_key, external_url) = match &asset.location {
        AssetLocation::Stored { storage_key } => (Some(storage_key.as_str()), None),
        AssetLocation::External { url } => (None, Some(url.as_str())),
    };
    sqlx::query(
        r#"
        INSERT INTO assets (
            id, tenant_id, media_type, storage_key, external_url, original_filename,
            mime_type, byte_size, width, height, duration_ms, sha256, thumbnail_key,
            visibility, status, metadata_json, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', ?, ?, ?)
        "#,
    )
    .bind(asset.id.to_string())
    .bind(asset.tenant_id.to_string())
    .bind(encode_enum(&asset.media_type)?)
    .bind(storage_key)
    .bind(external_url)
    .bind(&asset.original_filename)
    .bind(&asset.mime_type)
    .bind(optional_u64_to_i64("asset.byte_size", asset.byte_size)?)
    .bind(asset.width.map(i64::from))
    .bind(asset.height.map(i64::from))
    .bind(optional_u64_to_i64("asset.duration_ms", asset.duration_ms)?)
    .bind(&asset.sha256)
    .bind(&asset.thumbnail_key)
    .bind(encode_enum(&asset.visibility)?)
    .bind(encode_enum(&asset.status)?)
    .bind(format_timestamp(asset.timestamps.created_at)?)
    .bind(format_timestamp(asset.timestamps.updated_at)?)
    .bind(format_optional_timestamp(asset.timestamps.deleted_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_asset_file(
    connection: &mut SqliteConnection,
    file: &AssetFile,
) -> Result<(), SqliteRepositoryError> {
    sqlx::query(
        r#"
        INSERT INTO asset_files (
            id, tenant_id, asset_id, role, storage_key, mime_type, byte_size,
            width, height, sha256, generator_version, created_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(file.id.to_string())
    .bind(file.tenant_id.to_string())
    .bind(file.asset_id.to_string())
    .bind(encode_enum(&file.role)?)
    .bind(&file.storage_key)
    .bind(&file.mime_type)
    .bind(optional_u64_to_i64(
        "asset_file.byte_size",
        Some(file.byte_size),
    )?)
    .bind(file.width.map(i64::from))
    .bind(file.height.map(i64::from))
    .bind(&file.sha256)
    .bind(&file.generator_version)
    .bind(format_timestamp(file.created_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_generation_run(
    connection: &mut SqliteConnection,
    run: &GenerationRun,
) -> Result<(), SqliteRepositoryError> {
    let (cost_amount_micros, cost_currency) = run.cost.as_ref().map_or((None, None), |cost| {
        (Some(cost.amount_micros), Some(cost.currency.as_str()))
    });
    sqlx::query(
        r#"
        INSERT INTO generation_runs (
            id, tenant_id, variant_id, source, input_snapshot_json,
            parameters_snapshot_json, external_url, started_at, completed_at,
            status, failure_message, cost_amount_micros, cost_currency, created_by,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(run.id.to_string())
    .bind(run.tenant_id.to_string())
    .bind(run.variant_id.to_string())
    .bind(encode_enum(&run.source)?)
    .bind(serde_json::to_string(&run.input_snapshot)?)
    .bind(serde_json::to_string(&run.parameters_snapshot)?)
    .bind(&run.external_url)
    .bind(format_optional_timestamp(run.started_at)?)
    .bind(format_optional_timestamp(run.completed_at)?)
    .bind(encode_enum(&run.status)?)
    .bind(&run.failure_message)
    .bind(optional_u64_to_i64(
        "generation.cost.amount_micros",
        cost_amount_micros,
    )?)
    .bind(cost_currency)
    .bind(&run.created_by)
    .bind(format_timestamp(run.timestamps.created_at)?)
    .bind(format_timestamp(run.timestamps.updated_at)?)
    .bind(format_optional_timestamp(run.timestamps.deleted_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_asset_link(
    connection: &mut SqliteConnection,
    link: &AssetLink,
) -> Result<(), SqliteRepositoryError> {
    let created_at = format_timestamp(link.created_at)?;
    sqlx::query(
        r#"
        INSERT INTO asset_links (
            id, tenant_id, asset_id, generation_run_id, prompt_id, related_asset_id,
            relation_type, sort_order, note, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(link.id.to_string())
    .bind(link.tenant_id.to_string())
    .bind(link.asset_id.to_string())
    .bind(link.generation_run_id.map(|id| id.to_string()))
    .bind(link.prompt_id.map(|id| id.to_string()))
    .bind(link.related_asset_id.map(|id| id.to_string()))
    .bind(encode_enum(&link.relation_type)?)
    .bind(i64::from(link.sort_order))
    .bind(&link.note)
    .bind(&created_at)
    .bind(created_at)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_evaluation(
    connection: &mut SqliteConnection,
    evaluation: &Evaluation,
) -> Result<(), SqliteRepositoryError> {
    sqlx::query(
        r#"
        INSERT INTO evaluations (
            id, tenant_id, asset_id, generation_run_id, overall_score,
            is_featured, is_reusable, dimension_scores_json, strengths, issues,
            lesson, created_by, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(evaluation.id.to_string())
    .bind(evaluation.tenant_id.to_string())
    .bind(evaluation.asset_id.to_string())
    .bind(evaluation.generation_run_id.map(|id| id.to_string()))
    .bind(i64::from(evaluation.overall_score))
    .bind(evaluation.is_featured)
    .bind(evaluation.is_reusable)
    .bind(serde_json::to_string(&evaluation.dimension_scores)?)
    .bind(&evaluation.strengths)
    .bind(&evaluation.issues)
    .bind(&evaluation.lesson)
    .bind(&evaluation.created_by)
    .bind(format_timestamp(evaluation.timestamps.created_at)?)
    .bind(format_timestamp(evaluation.timestamps.updated_at)?)
    .bind(format_optional_timestamp(evaluation.timestamps.deleted_at)?)
    .execute(connection)
    .await?;
    Ok(())
}

fn optional_u64_to_i64(
    field: &'static str,
    value: Option<u64>,
) -> Result<Option<i64>, SqliteRepositoryError> {
    value
        .map(|value| {
            i64::try_from(value).map_err(|_| SqliteRepositoryError::InvalidData {
                field,
                value: value.to_string(),
            })
        })
        .transpose()
}

fn prompt_variant_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<PromptVariant, SqliteRepositoryError> {
    Ok(PromptVariant {
        id: parse_id("prompt_variants.id", row.try_get("id")?)?,
        tenant_id: parse_id("prompt_variants.tenant_id", row.try_get("tenant_id")?)?,
        prompt_id: parse_id("prompt_variants.prompt_id", row.try_get("prompt_id")?)?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        model_version: row.try_get("model_version")?,
        prompt_text: row.try_get("prompt_text")?,
        parameters: serde_json::from_str(&row.try_get::<String, _>("parameters_json")?)?,
        adaptation_notes: row.try_get("adaptation_notes")?,
        success_tips: row.try_get("success_tips")?,
        failure_patterns: row.try_get("failure_patterns")?,
        status: decode_enum("prompt_variants.status", row.try_get("status")?)?,
        revision: i64_to_u32("prompt_variants.revision", row.try_get("revision")?)?,
        timestamps: timestamps_from_row(row)?,
    })
}

fn project_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Project, SqliteRepositoryError> {
    Ok(Project {
        id: parse_id("projects.id", row.try_get("id")?)?,
        tenant_id: parse_id("projects.tenant_id", row.try_get("tenant_id")?)?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        cover_asset_id: parse_optional_id(
            "projects.cover_asset_id",
            row.try_get("cover_asset_id")?,
        )?,
        status: decode_enum("projects.status", row.try_get("status")?)?,
        timestamps: timestamps_from_row(row)?,
    })
}

fn prompt_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Prompt, SqliteRepositoryError> {
    Ok(Prompt {
        id: parse_id("prompts.id", row.try_get("id")?)?,
        tenant_id: parse_id("prompts.tenant_id", row.try_get("tenant_id")?)?,
        project_id: parse_id("prompts.project_id", row.try_get("project_id")?)?,
        title: row.try_get("title")?,
        creative_intent: row.try_get("creative_intent")?,
        base_prompt: row.try_get("base_prompt")?,
        negative_constraints: row.try_get("negative_constraints")?,
        status: decode_enum("prompts.status", row.try_get("status")?)?,
        current_version: i64_to_u32("prompts.current_version", row.try_get("current_version")?)?,
        created_by: row.try_get("created_by")?,
        timestamps: timestamps_from_row(row)?,
    })
}

fn asset_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Asset, SqliteRepositoryError> {
    let storage_key = row.try_get::<Option<String>, _>("storage_key")?;
    let external_url = row.try_get::<Option<String>, _>("external_url")?;
    let location = match (storage_key, external_url) {
        (Some(storage_key), None) => AssetLocation::Stored { storage_key },
        (None, Some(url)) => AssetLocation::External { url },
        (storage_key, external_url) => {
            return Err(SqliteRepositoryError::InvalidData {
                field: "assets.location",
                value: format!("storage_key={storage_key:?}, external_url={external_url:?}"),
            });
        }
    };
    Ok(Asset {
        id: parse_id("assets.id", row.try_get("id")?)?,
        tenant_id: parse_id("assets.tenant_id", row.try_get("tenant_id")?)?,
        media_type: decode_enum("assets.media_type", row.try_get("media_type")?)?,
        location,
        original_filename: row.try_get("original_filename")?,
        mime_type: row.try_get("mime_type")?,
        byte_size: optional_i64_to_u64("assets.byte_size", row.try_get("byte_size")?)?,
        width: optional_i64_to_u32("assets.width", row.try_get("width")?)?,
        height: optional_i64_to_u32("assets.height", row.try_get("height")?)?,
        duration_ms: optional_i64_to_u64("assets.duration_ms", row.try_get("duration_ms")?)?,
        sha256: row.try_get("sha256")?,
        thumbnail_key: row.try_get("thumbnail_key")?,
        status: decode_enum("assets.status", row.try_get("status")?)?,
        visibility: decode_enum("assets.visibility", row.try_get("visibility")?)?,
        timestamps: timestamps_from_row(row)?,
    })
}

fn generation_run_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GenerationRun, SqliteRepositoryError> {
    let amount = row.try_get::<Option<i64>, _>("cost_amount_micros")?;
    let currency = row.try_get::<Option<String>, _>("cost_currency")?;
    let cost = match (amount, currency) {
        (Some(amount), Some(currency)) => Some(GenerationCost {
            amount_micros: i64_to_u64("generation_runs.cost_amount_micros", amount)?,
            currency,
        }),
        (None, None) => None,
        (amount, currency) => {
            return Err(SqliteRepositoryError::InvalidData {
                field: "generation_runs.cost",
                value: format!("amount={amount:?}, currency={currency:?}"),
            });
        }
    };
    Ok(GenerationRun {
        id: parse_id("generation_runs.id", row.try_get("id")?)?,
        tenant_id: parse_id("generation_runs.tenant_id", row.try_get("tenant_id")?)?,
        variant_id: parse_id("generation_runs.variant_id", row.try_get("variant_id")?)?,
        source: decode_enum("generation_runs.source", row.try_get("source")?)?,
        input_snapshot: serde_json::from_str(&row.try_get::<String, _>("input_snapshot_json")?)?,
        parameters_snapshot: serde_json::from_str(
            &row.try_get::<String, _>("parameters_snapshot_json")?,
        )?,
        external_url: row.try_get("external_url")?,
        started_at: parse_optional_timestamp(
            "generation_runs.started_at",
            row.try_get("started_at")?,
        )?,
        completed_at: parse_optional_timestamp(
            "generation_runs.completed_at",
            row.try_get("completed_at")?,
        )?,
        status: decode_enum("generation_runs.status", row.try_get("status")?)?,
        failure_message: row.try_get("failure_message")?,
        cost,
        created_by: row.try_get("created_by")?,
        timestamps: timestamps_from_row(row)?,
    })
}

fn asset_link_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<AssetLink, SqliteRepositoryError> {
    Ok(AssetLink {
        id: parse_id("asset_links.id", row.try_get("id")?)?,
        tenant_id: parse_id("asset_links.tenant_id", row.try_get("tenant_id")?)?,
        asset_id: parse_id("asset_links.asset_id", row.try_get("asset_id")?)?,
        generation_run_id: parse_optional_id(
            "asset_links.generation_run_id",
            row.try_get("generation_run_id")?,
        )?,
        prompt_id: parse_optional_id("asset_links.prompt_id", row.try_get("prompt_id")?)?,
        related_asset_id: parse_optional_id(
            "asset_links.related_asset_id",
            row.try_get("related_asset_id")?,
        )?,
        relation_type: decode_enum("asset_links.relation_type", row.try_get("relation_type")?)?,
        sort_order: i64_to_u32("asset_links.sort_order", row.try_get("sort_order")?)?,
        note: row.try_get("note")?,
        created_at: parse_timestamp("asset_links.created_at", row.try_get("created_at")?)?,
    })
}

fn evaluation_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Evaluation, SqliteRepositoryError> {
    Ok(Evaluation {
        id: parse_id("evaluations.id", row.try_get("id")?)?,
        tenant_id: parse_id("evaluations.tenant_id", row.try_get("tenant_id")?)?,
        asset_id: parse_id("evaluations.asset_id", row.try_get("asset_id")?)?,
        generation_run_id: parse_optional_id(
            "evaluations.generation_run_id",
            row.try_get("generation_run_id")?,
        )?,
        overall_score: i64_to_u8("evaluations.overall_score", row.try_get("overall_score")?)?,
        is_featured: row.try_get("is_featured")?,
        is_reusable: row.try_get("is_reusable")?,
        dimension_scores: serde_json::from_str(
            &row.try_get::<String, _>("dimension_scores_json")?,
        )?,
        strengths: row.try_get("strengths")?,
        issues: row.try_get("issues")?,
        lesson: row.try_get("lesson")?,
        created_by: row.try_get("created_by")?,
        timestamps: timestamps_from_row(row)?,
    })
}

fn timestamps_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<RecordTimestamps, SqliteRepositoryError> {
    Ok(RecordTimestamps {
        created_at: parse_timestamp("created_at", row.try_get("created_at")?)?,
        updated_at: parse_timestamp("updated_at", row.try_get("updated_at")?)?,
        deleted_at: parse_optional_timestamp("deleted_at", row.try_get("deleted_at")?)?,
    })
}

fn i64_to_u8(field: &'static str, value: i64) -> Result<u8, SqliteRepositoryError> {
    u8::try_from(value).map_err(|_| SqliteRepositoryError::InvalidData {
        field,
        value: value.to_string(),
    })
}

fn i64_to_u32(field: &'static str, value: i64) -> Result<u32, SqliteRepositoryError> {
    u32::try_from(value).map_err(|_| SqliteRepositoryError::InvalidData {
        field,
        value: value.to_string(),
    })
}

fn i64_to_u64(field: &'static str, value: i64) -> Result<u64, SqliteRepositoryError> {
    u64::try_from(value).map_err(|_| SqliteRepositoryError::InvalidData {
        field,
        value: value.to_string(),
    })
}

fn optional_i64_to_u32(
    field: &'static str,
    value: Option<i64>,
) -> Result<Option<u32>, SqliteRepositoryError> {
    value.map(|value| i64_to_u32(field, value)).transpose()
}

fn optional_i64_to_u64(
    field: &'static str,
    value: Option<i64>,
) -> Result<Option<u64>, SqliteRepositoryError> {
    value.map(|value| i64_to_u64(field, value)).transpose()
}

fn parse_optional_id<T>(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<T>, SqliteRepositoryError>
where
    T: std::str::FromStr,
{
    value.map(|value| parse_id(field, value)).transpose()
}

fn database_error(error: sqlx::Error) -> RepositoryError {
    repository_error(SqliteRepositoryError::Database(error))
}

fn repository_error(error: SqliteRepositoryError) -> RepositoryError {
    match error {
        SqliteRepositoryError::Database(error) => {
            if let Some(database_error) = error.as_database_error()
                && (database_error.is_unique_violation()
                    || database_error.is_foreign_key_violation())
            {
                return RepositoryError::Conflict(database_error.message().into());
            }
            RepositoryError::Unavailable(error.to_string())
        }
        SqliteRepositoryError::InvalidData { field, value } => {
            RepositoryError::CorruptData(format!("{field}: {value}"))
        }
        SqliteRepositoryError::Json(error) => RepositoryError::CorruptData(error.to_string()),
        SqliteRepositoryError::TenantMismatch {
            requested_tenant,
            entity_tenant,
        } => RepositoryError::Other(format!(
            "entity tenant {entity_tenant} does not match requested tenant {requested_tenant}"
        )),
        other => RepositoryError::Unavailable(other.to_string()),
    }
}

// Implemented below in focused sections to keep the port implementation easy
// to compare with its product-level use cases.
async fn search_library(
    repository: &SqliteLibraryRepository,
    tenant_id: TenantId,
    criteria: &SearchCriteria,
) -> Result<SearchPage<SearchRecord>, RepositoryError> {
    let limit = criteria.limit.clamp(1, 100) as usize;
    let fetch_limit =
        i64::try_from(limit + 1).map_err(|error| RepositoryError::Other(error.to_string()))?;
    let cursor = criteria
        .cursor
        .as_deref()
        .map(parse_search_cursor)
        .transpose()?;
    let all_kinds = criteria.kinds.is_empty();
    let mut records = Vec::with_capacity((limit + 1) * 2);

    if all_kinds || criteria.kinds.contains(&SearchKind::Prompt) {
        let mut query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT p.id, p.tenant_id, p.title, NULLIF(p.creative_intent, '') AS summary,
                   p.project_id, project.name AS project_name,
                   (SELECT pv.provider
                      FROM prompt_variants pv
                     WHERE pv.tenant_id = p.tenant_id AND pv.prompt_id = p.id
                       AND pv.deleted_at IS NULL
                     ORDER BY pv.updated_at DESC, pv.id ASC LIMIT 1) AS provider,
                   (SELECT pv.model
                      FROM prompt_variants pv
                     WHERE pv.tenant_id = p.tenant_id AND pv.prompt_id = p.id
                       AND pv.deleted_at IS NULL
                     ORDER BY pv.updated_at DESC, pv.id ASC LIMIT 1) AS model,
                   COALESCE((
                     SELECT json_group_array(t.name)
                       FROM prompt_tags pt
                       JOIN tags t ON t.tenant_id = pt.tenant_id AND t.id = pt.tag_id
                      WHERE pt.tenant_id = p.tenant_id AND pt.prompt_id = p.id
                        AND t.deleted_at IS NULL
                   ), '[]') AS tags_json,
                   p.created_at
              FROM prompts p
              JOIN projects project
                ON project.tenant_id = p.tenant_id AND project.id = p.project_id
             WHERE p.tenant_id = "#,
        );
        query
            .push_bind(tenant_id.to_string())
            .push(" AND p.deleted_at IS NULL");
        if !criteria.include_archived {
            query.push(" AND p.status <> 'archived'");
        }
        if let Some(project_id) = criteria.project_id {
            query
                .push(" AND p.project_id = ")
                .push_bind(project_id.to_string());
        }
        if !criteria.prompt_statuses.is_empty() {
            query.push(" AND p.status IN (");
            let mut values = query.separated(", ");
            for status in &criteria.prompt_statuses {
                values.push_bind(encode_enum(status).map_err(repository_error)?);
            }
            values.push_unseparated(")");
        }
        push_prompt_variant_filter(&mut query, "provider", &criteria.providers);
        push_prompt_variant_filter(&mut query, "model", &criteria.models);
        if let Some(search_term) = normalized_search_term(criteria.query.as_deref()) {
            query.push(" AND (p.title LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(" OR p.creative_intent LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(" OR p.base_prompt LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(
                " OR EXISTS (SELECT 1 FROM prompt_variants search_variant \
                 WHERE search_variant.tenant_id = p.tenant_id \
                   AND search_variant.prompt_id = p.id \
                   AND search_variant.deleted_at IS NULL \
                   AND search_variant.prompt_text LIKE ",
            );
            query.push_bind(search_term).push(" ESCAPE '\\'))");
        }
        for tag in &criteria.tags {
            query.push(
                " AND EXISTS (SELECT 1 FROM prompt_tags pt \
                 JOIN tags t ON t.tenant_id = pt.tenant_id AND t.id = pt.tag_id \
                 WHERE pt.tenant_id = p.tenant_id AND pt.prompt_id = p.id \
                   AND t.deleted_at IS NULL AND t.name = ",
            );
            query.push_bind(tag.trim().to_owned()).push(")");
        }
        if let Some(cursor) = &cursor {
            push_cursor_filter(&mut query, "p", SearchKind::Prompt, cursor);
        }
        query
            .push(" ORDER BY p.created_at DESC, p.id ASC LIMIT ")
            .push_bind(fetch_limit);

        let rows = query
            .build()
            .fetch_all(&repository.pool)
            .await
            .map_err(database_error)?;
        for row in rows {
            records.push(prompt_search_record_from_row(&row).map_err(repository_error)?);
        }
    }

    if all_kinds || criteria.kinds.contains(&SearchKind::Asset) {
        let mut query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT a.id, a.tenant_id,
                   COALESCE(a.original_filename, a.media_type || ' asset') AS title,
                   (SELECT p.title
                      FROM asset_links al
                      JOIN prompts p
                        ON p.tenant_id = al.tenant_id AND p.id = al.prompt_id
                       AND p.deleted_at IS NULL
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1) AS summary,
                   (SELECT p.project_id
                      FROM asset_links al
                      JOIN prompts p
                        ON p.tenant_id = al.tenant_id AND p.id = al.prompt_id
                       AND p.deleted_at IS NULL
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1) AS project_id,
                   (SELECT project.name
                      FROM asset_links al
                      JOIN prompts p
                        ON p.tenant_id = al.tenant_id AND p.id = al.prompt_id
                       AND p.deleted_at IS NULL
                      JOIN projects project
                        ON project.tenant_id = p.tenant_id AND project.id = p.project_id
                       AND project.deleted_at IS NULL
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1) AS project_name,
                   a.media_type,
                   a.thumbnail_key,
                   (SELECT pv.provider
                      FROM asset_links al
                      JOIN generation_runs gr
                        ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
                       AND gr.deleted_at IS NULL
                      JOIN prompt_variants pv
                        ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                       AND pv.deleted_at IS NULL
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1) AS provider,
                   (SELECT pv.model
                      FROM asset_links al
                      JOIN generation_runs gr
                        ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
                       AND gr.deleted_at IS NULL
                      JOIN prompt_variants pv
                        ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                       AND pv.deleted_at IS NULL
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1) AS model,
                   (SELECT e.overall_score
                      FROM evaluations e
                     WHERE e.tenant_id = a.tenant_id AND e.asset_id = a.id
                       AND e.deleted_at IS NULL
                     ORDER BY e.created_at DESC, e.id ASC LIMIT 1) AS overall_score,
                   EXISTS(SELECT 1 FROM evaluations e
                           WHERE e.tenant_id = a.tenant_id AND e.asset_id = a.id
                             AND e.deleted_at IS NULL AND e.is_featured = 1) AS is_featured,
                   COALESCE((
                     SELECT json_group_array(t.name)
                       FROM asset_tags atag
                       JOIN tags t ON t.tenant_id = atag.tenant_id AND t.id = atag.tag_id
                      WHERE atag.tenant_id = a.tenant_id AND atag.asset_id = a.id
                        AND t.deleted_at IS NULL
                   ), '[]') AS tags_json,
                   a.created_at
              FROM assets a
             WHERE a.tenant_id = "#,
        );
        query
            .push_bind(tenant_id.to_string())
            .push(" AND a.deleted_at IS NULL");
        if !criteria.include_archived {
            query.push(" AND a.status <> 'archived'");
        }
        if !criteria.media_types.is_empty() {
            query.push(" AND a.media_type IN (");
            let mut values = query.separated(", ");
            for media_type in &criteria.media_types {
                values.push_bind(encode_enum(media_type).map_err(repository_error)?);
            }
            values.push_unseparated(")");
        }
        if let Some(project_id) = criteria.project_id {
            push_asset_project_filter(&mut query, project_id);
        }
        push_asset_variant_filter(&mut query, "provider", &criteria.providers);
        push_asset_variant_filter(&mut query, "model", &criteria.models);
        if let Some(minimum_score) = criteria.minimum_score {
            query.push(
                " AND EXISTS (SELECT 1 FROM evaluations score_eval \
                 WHERE score_eval.tenant_id = a.tenant_id AND score_eval.asset_id = a.id \
                   AND score_eval.deleted_at IS NULL AND score_eval.overall_score >= ",
            );
            query.push_bind(i64::from(minimum_score)).push(")");
        }
        if criteria.featured_only {
            query.push(
                " AND EXISTS (SELECT 1 FROM evaluations featured_eval \
                 WHERE featured_eval.tenant_id = a.tenant_id \
                   AND featured_eval.asset_id = a.id \
                   AND featured_eval.deleted_at IS NULL \
                   AND featured_eval.is_featured = 1)",
            );
        }
        if let Some(search_term) = normalized_search_term(criteria.query.as_deref()) {
            query.push(" AND (a.original_filename LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(" OR a.mime_type LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(" OR a.storage_key LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(" OR a.external_url LIKE ");
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(
                " OR EXISTS (SELECT 1 FROM asset_links search_link \
                 JOIN prompts search_prompt \
                   ON search_prompt.tenant_id = search_link.tenant_id \
                  AND search_prompt.id = search_link.prompt_id \
                  AND search_prompt.deleted_at IS NULL \
                 WHERE search_link.tenant_id = a.tenant_id \
                   AND search_link.asset_id = a.id \
                   AND search_link.deleted_at IS NULL \
                   AND (search_prompt.title LIKE ",
            );
            query.push_bind(search_term.clone()).push(" ESCAPE '\\'");
            query.push(" OR search_prompt.base_prompt LIKE ");
            query.push_bind(search_term).push(" ESCAPE '\\')))");
        }
        for tag in &criteria.tags {
            query.push(
                " AND EXISTS (SELECT 1 FROM asset_tags atag \
                 JOIN tags t ON t.tenant_id = atag.tenant_id AND t.id = atag.tag_id \
                 WHERE atag.tenant_id = a.tenant_id AND atag.asset_id = a.id \
                   AND t.deleted_at IS NULL AND t.name = ",
            );
            query.push_bind(tag.trim().to_owned()).push(")");
        }
        if let Some(cursor) = &cursor {
            push_cursor_filter(&mut query, "a", SearchKind::Asset, cursor);
        }
        query
            .push(" ORDER BY a.created_at DESC, a.id ASC LIMIT ")
            .push_bind(fetch_limit);

        let rows = query
            .build()
            .fetch_all(&repository.pool)
            .await
            .map_err(database_error)?;
        for row in rows {
            records.push(asset_search_record_from_row(&row).map_err(repository_error)?);
        }
    }

    records.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| {
                search_kind_rank(record_kind(left)).cmp(&search_kind_rank(record_kind(right)))
            })
            .then_with(|| record_id_text(left).cmp(&record_id_text(right)))
    });
    let has_more = records.len() > limit;
    records.truncate(limit);
    let next_cursor = has_more
        .then(|| records.last().map(format_search_cursor))
        .flatten();

    Ok(SearchPage {
        items: records,
        next_cursor,
    })
}

#[derive(Debug)]
struct SearchCursor {
    created_at_db: String,
    kind: SearchKind,
    id: String,
}

fn parse_search_cursor(value: &str) -> Result<SearchCursor, RepositoryError> {
    let mut parts = value.splitn(3, '|');
    let created_at_db = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| RepositoryError::Other("invalid search cursor timestamp".into()))?
        .to_owned();
    let kind = match parts.next() {
        Some("prompt") => SearchKind::Prompt,
        Some("asset") => SearchKind::Asset,
        _ => return Err(RepositoryError::Other("invalid search cursor kind".into())),
    };
    let id = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| RepositoryError::Other("invalid search cursor id".into()))?
        .to_owned();
    parse_timestamp("search.cursor.created_at", created_at_db.clone()).map_err(repository_error)?;
    Ok(SearchCursor {
        created_at_db,
        kind,
        id,
    })
}

fn format_search_cursor(record: &SearchRecord) -> String {
    let timestamp = format_timestamp(record.created_at)
        .expect("valid domain timestamps are always representable as RFC 3339");
    let kind = match record_kind(record) {
        SearchKind::Prompt => "prompt",
        SearchKind::Asset => "asset",
    };
    format!("{timestamp}|{kind}|{}", record_id_text(record))
}

fn push_cursor_filter(
    query: &mut QueryBuilder<Sqlite>,
    table_alias: &str,
    table_kind: SearchKind,
    cursor: &SearchCursor,
) {
    let table_rank = search_kind_rank(table_kind);
    let cursor_rank = search_kind_rank(cursor.kind);
    query
        .push(" AND (")
        .push(table_alias)
        .push(".created_at < ")
        .push_bind(cursor.created_at_db.clone());
    if table_rank > cursor_rank {
        query
            .push(" OR ")
            .push(table_alias)
            .push(".created_at = ")
            .push_bind(cursor.created_at_db.clone());
    } else if table_rank == cursor_rank {
        query
            .push(" OR (")
            .push(table_alias)
            .push(".created_at = ")
            .push_bind(cursor.created_at_db.clone())
            .push(" AND ")
            .push(table_alias)
            .push(".id > ")
            .push_bind(cursor.id.clone())
            .push(")");
    }
    query.push(")");
}

fn push_prompt_variant_filter(
    query: &mut QueryBuilder<Sqlite>,
    column: &'static str,
    values: &[String],
) {
    if values.is_empty() {
        return;
    }
    query.push(
        " AND EXISTS (SELECT 1 FROM prompt_variants filter_variant \
         WHERE filter_variant.tenant_id = p.tenant_id \
           AND filter_variant.prompt_id = p.id \
           AND filter_variant.deleted_at IS NULL AND filter_variant.",
    );
    query.push(column).push(" IN (");
    let mut separated = query.separated(", ");
    for value in values {
        separated.push_bind(value.trim().to_owned());
    }
    separated.push_unseparated("))");
}

fn push_asset_variant_filter(
    query: &mut QueryBuilder<Sqlite>,
    column: &'static str,
    values: &[String],
) {
    if values.is_empty() {
        return;
    }
    query.push(
        " AND EXISTS (SELECT 1 FROM asset_links filter_link \
         LEFT JOIN generation_runs filter_run \
           ON filter_run.tenant_id = filter_link.tenant_id \
          AND filter_run.id = filter_link.generation_run_id \
          AND filter_run.deleted_at IS NULL \
         LEFT JOIN prompt_variants filter_variant \
           ON filter_variant.tenant_id = filter_run.tenant_id \
          AND filter_variant.id = filter_run.variant_id \
          AND filter_variant.deleted_at IS NULL \
         LEFT JOIN prompt_variants direct_variant \
           ON direct_variant.tenant_id = filter_link.tenant_id \
          AND direct_variant.prompt_id = filter_link.prompt_id \
          AND direct_variant.deleted_at IS NULL \
         WHERE filter_link.tenant_id = a.tenant_id \
           AND filter_link.asset_id = a.id \
           AND filter_link.deleted_at IS NULL AND COALESCE(filter_variant.",
    );
    query
        .push(column)
        .push(", direct_variant.")
        .push(column)
        .push(") IN (");
    let mut separated = query.separated(", ");
    for value in values {
        separated.push_bind(value.trim().to_owned());
    }
    separated.push_unseparated("))");
}

fn push_asset_project_filter(query: &mut QueryBuilder<Sqlite>, project_id: ProjectId) {
    query.push(
        " AND EXISTS (SELECT 1 FROM asset_links project_link \
         LEFT JOIN prompts direct_prompt \
           ON direct_prompt.tenant_id = project_link.tenant_id \
          AND direct_prompt.id = project_link.prompt_id \
          AND direct_prompt.deleted_at IS NULL \
         LEFT JOIN generation_runs project_run \
           ON project_run.tenant_id = project_link.tenant_id \
          AND project_run.id = project_link.generation_run_id \
          AND project_run.deleted_at IS NULL \
         LEFT JOIN prompt_variants project_variant \
           ON project_variant.tenant_id = project_run.tenant_id \
          AND project_variant.id = project_run.variant_id \
          AND project_variant.deleted_at IS NULL \
         LEFT JOIN prompts generated_prompt \
           ON generated_prompt.tenant_id = project_variant.tenant_id \
          AND generated_prompt.id = project_variant.prompt_id \
          AND generated_prompt.deleted_at IS NULL \
         WHERE project_link.tenant_id = a.tenant_id \
           AND project_link.asset_id = a.id \
           AND project_link.deleted_at IS NULL \
           AND COALESCE(direct_prompt.project_id, generated_prompt.project_id) = ",
    );
    query.push_bind(project_id.to_string()).push(")");
}

fn normalized_search_term(query: Option<&str>) -> Option<String> {
    query.and_then(|query| {
        let query = query.trim();
        if query.is_empty() {
            None
        } else {
            let escaped = query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            Some(format!("%{escaped}%"))
        }
    })
}

fn prompt_search_record_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<SearchRecord, SqliteRepositoryError> {
    Ok(SearchRecord {
        tenant_id: parse_id("prompts.tenant_id", row.try_get("tenant_id")?)?,
        id: SearchRecordId::Prompt(parse_id("prompts.id", row.try_get("id")?)?),
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        project_id: Some(parse_id("prompts.project_id", row.try_get("project_id")?)?),
        project_name: row.try_get("project_name")?,
        media_type: None,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        thumbnail_key: None,
        overall_score: None,
        is_featured: false,
        tags: serde_json::from_str(&row.try_get::<String, _>("tags_json")?)?,
        created_at: parse_timestamp("prompts.created_at", row.try_get("created_at")?)?,
    })
}

fn asset_search_record_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<SearchRecord, SqliteRepositoryError> {
    let overall_score = row
        .try_get::<Option<i64>, _>("overall_score")?
        .map(|score| i64_to_u8("evaluations.overall_score", score))
        .transpose()?;
    Ok(SearchRecord {
        tenant_id: parse_id("assets.tenant_id", row.try_get("tenant_id")?)?,
        id: SearchRecordId::Asset(parse_id("assets.id", row.try_get("id")?)?),
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        project_id: parse_optional_id("assets.project_id", row.try_get("project_id")?)?,
        project_name: row.try_get("project_name")?,
        media_type: Some(decode_enum(
            "assets.media_type",
            row.try_get("media_type")?,
        )?),
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        thumbnail_key: row.try_get("thumbnail_key")?,
        overall_score,
        is_featured: row.try_get("is_featured")?,
        tags: serde_json::from_str(&row.try_get::<String, _>("tags_json")?)?,
        created_at: parse_timestamp("assets.created_at", row.try_get("created_at")?)?,
    })
}

const fn search_kind_rank(kind: SearchKind) -> u8 {
    match kind {
        SearchKind::Prompt => 0,
        SearchKind::Asset => 1,
    }
}

const fn record_kind(record: &SearchRecord) -> SearchKind {
    match record.id {
        SearchRecordId::Prompt(_) => SearchKind::Prompt,
        SearchRecordId::Asset(_) => SearchKind::Asset,
    }
}

fn record_id_text(record: &SearchRecord) -> String {
    match record.id {
        SearchRecordId::Prompt(id) => id.to_string(),
        SearchRecordId::Asset(id) => id.to_string(),
    }
}

async fn get_library_overview(
    repository: &SqliteLibraryRepository,
    tenant_id: TenantId,
) -> Result<LibraryOverviewSnapshot, RepositoryError> {
    let project_rows = sqlx::query(
        r#"
        SELECT id, tenant_id, name, description, cover_asset_id, status,
               created_at, updated_at, deleted_at
          FROM projects
         WHERE tenant_id = ? AND deleted_at IS NULL
         ORDER BY updated_at DESC, id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let projects = project_rows
        .iter()
        .map(project_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let prompt_rows = sqlx::query(
        r#"
        SELECT id, tenant_id, project_id, title, creative_intent, base_prompt,
               negative_constraints, status, current_version, created_by,
               created_at, updated_at, deleted_at
          FROM prompts
         WHERE tenant_id = ? AND deleted_at IS NULL
         ORDER BY updated_at DESC, id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let prompts = prompt_rows
        .iter()
        .map(prompt_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let variant_rows = sqlx::query(
        r#"
        SELECT id, tenant_id, prompt_id, provider, model, model_version,
               prompt_text, parameters_json, adaptation_notes, success_tips,
               failure_patterns, status, revision, created_at, updated_at, deleted_at
          FROM prompt_variants
         WHERE tenant_id = ? AND deleted_at IS NULL
         ORDER BY updated_at DESC, id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let variants = variant_rows
        .iter()
        .map(prompt_variant_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let count_rows = sqlx::query(
        r#"
        SELECT gr.variant_id, COUNT(DISTINCT al.asset_id) AS result_count
          FROM generation_runs gr
          LEFT JOIN asset_links al
            ON al.tenant_id = gr.tenant_id AND al.generation_run_id = gr.id
           AND al.relation_type = 'output' AND al.deleted_at IS NULL
         WHERE gr.tenant_id = ? AND gr.deleted_at IS NULL
         GROUP BY gr.variant_id
        "#,
    )
    .bind(tenant_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let variant_result_counts = count_rows
        .iter()
        .map(|row| {
            Ok((
                parse_id("generation_runs.variant_id", row.try_get("variant_id")?)?,
                i64_to_u64("generation_runs.result_count", row.try_get("result_count")?)?,
            ))
        })
        .collect::<Result<BTreeMap<PromptVariantId, u64>, SqliteRepositoryError>>()
        .map_err(repository_error)?;

    Ok(LibraryOverviewSnapshot {
        projects,
        prompts,
        variants,
        variant_result_counts,
    })
}

async fn get_prompt_detail(
    repository: &SqliteLibraryRepository,
    tenant_id: TenantId,
    prompt_id: PromptId,
) -> Result<Option<PromptDetailSnapshot>, RepositoryError> {
    let prompt_row = sqlx::query(
        r#"
        SELECT id, tenant_id, project_id, title, creative_intent, base_prompt,
               negative_constraints, status, current_version, created_by,
               created_at, updated_at, deleted_at
          FROM prompts
         WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(prompt_id.to_string())
    .fetch_optional(&repository.pool)
    .await
    .map_err(database_error)?;
    let Some(prompt_row) = prompt_row else {
        return Ok(None);
    };
    let prompt = prompt_from_row(&prompt_row).map_err(repository_error)?;

    let project_row = sqlx::query(
        r#"
        SELECT id, tenant_id, name, description, cover_asset_id, status,
               created_at, updated_at, deleted_at
          FROM projects
         WHERE tenant_id = ? AND id = ?
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(prompt.project_id.to_string())
    .fetch_optional(&repository.pool)
    .await
    .map_err(database_error)?
    .ok_or_else(|| {
        RepositoryError::CorruptData(format!(
            "prompt {} references missing project {}",
            prompt.id, prompt.project_id
        ))
    })?;
    let project = project_from_row(&project_row).map_err(repository_error)?;

    let variant_rows = sqlx::query(
        r#"
        SELECT id, tenant_id, prompt_id, provider, model, model_version,
               prompt_text, parameters_json, adaptation_notes, success_tips,
               failure_patterns, status, revision, created_at, updated_at, deleted_at
          FROM prompt_variants
         WHERE tenant_id = ? AND prompt_id = ? AND deleted_at IS NULL
         ORDER BY updated_at DESC, id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(prompt_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let variants = variant_rows
        .iter()
        .map(prompt_variant_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let run_rows = sqlx::query(
        r#"
        SELECT gr.id, gr.tenant_id, gr.variant_id, gr.source,
               gr.input_snapshot_json, gr.parameters_snapshot_json, gr.external_url,
               gr.started_at, gr.completed_at, gr.status, gr.failure_message,
               gr.cost_amount_micros, gr.cost_currency, gr.created_by,
               gr.created_at, gr.updated_at, gr.deleted_at
          FROM generation_runs gr
          JOIN prompt_variants pv
            ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
         WHERE gr.tenant_id = ? AND pv.prompt_id = ? AND gr.deleted_at IS NULL
         ORDER BY gr.created_at DESC, gr.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(prompt_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let generation_runs = run_rows
        .iter()
        .map(generation_run_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let asset_rows = sqlx::query(
        r#"
        SELECT DISTINCT a.id, a.tenant_id, a.media_type, a.storage_key,
               a.external_url, a.original_filename, a.mime_type, a.byte_size,
               a.width, a.height, a.duration_ms, a.sha256, a.thumbnail_key,
               a.visibility, a.status, a.created_at, a.updated_at, a.deleted_at
          FROM assets a
         WHERE a.tenant_id = ? AND a.deleted_at IS NULL
           AND EXISTS (
               SELECT 1
                 FROM asset_links al
                 LEFT JOIN generation_runs gr
                   ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
                 LEFT JOIN prompt_variants pv
                   ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                  AND al.deleted_at IS NULL
                  AND (al.prompt_id = ? OR pv.prompt_id = ?)
           )
         ORDER BY a.created_at DESC, a.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(prompt_id.to_string())
    .bind(prompt_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let assets = asset_rows
        .iter()
        .map(asset_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let evaluation_rows = sqlx::query(
        r#"
        SELECT e.id, e.tenant_id, e.asset_id, e.generation_run_id,
               e.overall_score, e.is_featured, e.is_reusable,
               e.dimension_scores_json, e.strengths, e.issues, e.lesson,
               e.created_by, e.created_at, e.updated_at, e.deleted_at
          FROM evaluations e
         WHERE e.tenant_id = ? AND e.deleted_at IS NULL
           AND (
               EXISTS (
                   SELECT 1 FROM asset_links al
                   LEFT JOIN generation_runs gr
                     ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
                   LEFT JOIN prompt_variants pv
                     ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                  WHERE al.tenant_id = e.tenant_id AND al.asset_id = e.asset_id
                    AND al.deleted_at IS NULL
                    AND (al.prompt_id = ? OR pv.prompt_id = ?)
               )
               OR EXISTS (
                   SELECT 1 FROM generation_runs gr
                   JOIN prompt_variants pv
                     ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                  WHERE gr.tenant_id = e.tenant_id AND gr.id = e.generation_run_id
                    AND pv.prompt_id = ?
               )
           )
         ORDER BY e.created_at DESC, e.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(prompt_id.to_string())
    .bind(prompt_id.to_string())
    .bind(prompt_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let evaluations = evaluation_rows
        .iter()
        .map(evaluation_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    Ok(Some(PromptDetailSnapshot {
        project,
        prompt,
        variants,
        generation_runs,
        assets,
        evaluations,
    }))
}

async fn get_asset_detail(
    repository: &SqliteLibraryRepository,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Option<AssetDetailSnapshot>, RepositoryError> {
    let asset_row = sqlx::query(
        r#"
        SELECT id, tenant_id, media_type, storage_key, external_url,
               original_filename, mime_type, byte_size, width, height,
               duration_ms, sha256, thumbnail_key, visibility, status,
               created_at, updated_at, deleted_at
          FROM assets
         WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_optional(&repository.pool)
    .await
    .map_err(database_error)?;
    let Some(asset_row) = asset_row else {
        return Ok(None);
    };
    let asset = asset_from_row(&asset_row).map_err(repository_error)?;

    let link_rows = sqlx::query(
        r#"
        SELECT id, tenant_id, asset_id, generation_run_id, prompt_id,
               related_asset_id, relation_type, sort_order, note, created_at
          FROM asset_links
         WHERE tenant_id = ? AND asset_id = ? AND deleted_at IS NULL
         ORDER BY sort_order ASC, created_at ASC, id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let links = link_rows
        .iter()
        .map(asset_link_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let prompt_rows = sqlx::query(
        r#"
        SELECT DISTINCT p.id, p.tenant_id, p.project_id, p.title,
               p.creative_intent, p.base_prompt, p.negative_constraints,
               p.status, p.current_version, p.created_by,
               p.created_at, p.updated_at, p.deleted_at
          FROM prompts p
         WHERE p.tenant_id = ? AND p.deleted_at IS NULL
           AND EXISTS (
               SELECT 1 FROM asset_links al
               LEFT JOIN generation_runs gr
                 ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
               LEFT JOIN prompt_variants pv
                 ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
              WHERE al.tenant_id = p.tenant_id AND al.asset_id = ?
                AND al.deleted_at IS NULL
                AND (al.prompt_id = p.id OR pv.prompt_id = p.id)
           )
         ORDER BY p.created_at DESC, p.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let prompts = prompt_rows
        .iter()
        .map(prompt_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let variant_rows = sqlx::query(
        r#"
        SELECT DISTINCT pv.id, pv.tenant_id, pv.prompt_id, pv.provider,
               pv.model, pv.model_version, pv.prompt_text, pv.parameters_json,
               pv.adaptation_notes, pv.success_tips, pv.failure_patterns,
               pv.status, pv.revision, pv.created_at, pv.updated_at, pv.deleted_at
          FROM prompt_variants pv
         WHERE pv.tenant_id = ? AND pv.deleted_at IS NULL
           AND EXISTS (
               SELECT 1 FROM asset_links al
               LEFT JOIN generation_runs gr
                 ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
              WHERE al.tenant_id = pv.tenant_id AND al.asset_id = ?
                AND al.deleted_at IS NULL
                AND (gr.variant_id = pv.id OR al.prompt_id = pv.prompt_id)
           )
         ORDER BY pv.updated_at DESC, pv.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let variants = variant_rows
        .iter()
        .map(prompt_variant_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let run_rows = sqlx::query(
        r#"
        SELECT DISTINCT gr.id, gr.tenant_id, gr.variant_id, gr.source,
               gr.input_snapshot_json, gr.parameters_snapshot_json, gr.external_url,
               gr.started_at, gr.completed_at, gr.status, gr.failure_message,
               gr.cost_amount_micros, gr.cost_currency, gr.created_by,
               gr.created_at, gr.updated_at, gr.deleted_at
          FROM generation_runs gr
          JOIN asset_links al
            ON al.tenant_id = gr.tenant_id AND al.generation_run_id = gr.id
           AND al.deleted_at IS NULL
         WHERE gr.tenant_id = ? AND al.asset_id = ? AND gr.deleted_at IS NULL
         ORDER BY gr.created_at DESC, gr.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let generation_runs = run_rows
        .iter()
        .map(generation_run_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let evaluation_rows = sqlx::query(
        r#"
        SELECT id, tenant_id, asset_id, generation_run_id, overall_score,
               is_featured, is_reusable, dimension_scores_json, strengths,
               issues, lesson, created_by, created_at, updated_at, deleted_at
          FROM evaluations
         WHERE tenant_id = ? AND asset_id = ? AND deleted_at IS NULL
         ORDER BY created_at DESC, id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let evaluations = evaluation_rows
        .iter()
        .map(evaluation_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;

    let related_rows = sqlx::query(
        r#"
        SELECT DISTINCT related.id, related.tenant_id, related.media_type,
               related.storage_key, related.external_url, related.original_filename,
               related.mime_type, related.byte_size, related.width, related.height,
               related.duration_ms, related.sha256, related.thumbnail_key,
               related.visibility, related.status, related.created_at,
               related.updated_at, related.deleted_at
          FROM assets related
         WHERE related.tenant_id = ? AND related.deleted_at IS NULL
           AND related.id <> ?
           AND (
               EXISTS (
                   SELECT 1 FROM asset_links outgoing
                    WHERE outgoing.tenant_id = related.tenant_id
                      AND outgoing.asset_id = ?
                      AND outgoing.related_asset_id = related.id
                      AND outgoing.deleted_at IS NULL
               )
               OR EXISTS (
                   SELECT 1 FROM asset_links incoming
                    WHERE incoming.tenant_id = related.tenant_id
                      AND incoming.asset_id = related.id
                      AND incoming.related_asset_id = ?
                      AND incoming.deleted_at IS NULL
               )
           )
         ORDER BY related.created_at DESC, related.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .bind(asset_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let related_assets = related_rows
        .iter()
        .map(asset_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    let tag_rows = sqlx::query(
        r#"
        SELECT t.name
          FROM asset_tags atag
          JOIN tags t ON t.tenant_id = atag.tenant_id AND t.id = atag.tag_id
         WHERE atag.tenant_id = ? AND atag.asset_id = ? AND t.deleted_at IS NULL
         ORDER BY t.name COLLATE NOCASE ASC, t.id ASC
        "#,
    )
    .bind(tenant_id.to_string())
    .bind(asset_id.to_string())
    .fetch_all(&repository.pool)
    .await
    .map_err(database_error)?;
    let tags = tag_rows
        .iter()
        .map(|row| row.try_get("name"))
        .collect::<Result<Vec<String>, _>>()
        .map_err(database_error)?;

    Ok(Some(AssetDetailSnapshot {
        asset,
        links,
        prompts,
        variants,
        generation_runs,
        evaluations,
        related_assets,
        tags,
    }))
}
