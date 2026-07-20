#![allow(clippy::needless_raw_string_hashes, clippy::too_many_lines)]

//! Optional Turso Database adapter.
//!
//! This adapter uses the native-Rust `turso` engine for an embedded local
//! database. `SQLite` remains the zero-configuration default. The schema and
//! stable identifiers are shared so an explicit export/import path can move a
//! library between adapters.

use std::{
    cmp::Reverse,
    collections::BTreeSet,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use pah_application::{
    AssetContextReplacement, AssetDetailSnapshot, LibraryOverviewSnapshot, LibraryRepository,
    PromptDetailSnapshot, RepositoryError, SearchCriteria, SearchKind, SearchPage, SearchRecord,
    SearchRecordId,
};
use pah_domain::{
    Asset, AssetFile, AssetId, AssetLink, AssetLocation, Evaluation, GenerationCost, GenerationRun,
    Project, ProjectId, Prompt, PromptId, PromptVariant, PromptVariantId, TagId, Tenant, TenantId,
};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use turso::{Connection, Value, params_from_iter};

const INITIAL_SCHEMA: &str = include_str!("../../../migrations/sqlite/0001_initial.sql");
const ASSET_FILES_SCHEMA: &str = include_str!("../../../migrations/sqlite/0002_asset_files.sql");

#[derive(Debug, Error)]
pub enum TursoRepositoryError {
    #[error("cannot prepare Turso database directory {path}: {source}")]
    PrepareDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Turso database operation failed: {0}")]
    Database(#[from] turso::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("stored {field} value is invalid: {value}")]
    InvalidData { field: &'static str, value: String },
    #[error("entity tenant {entity_tenant} does not match requested tenant {requested_tenant}")]
    TenantMismatch {
        requested_tenant: TenantId,
        entity_tenant: TenantId,
    },
}

#[derive(Debug, Clone)]
pub struct TursoLibraryRepository {
    connection: Connection,
}

impl TursoLibraryRepository {
    /// Opens a local database with the native Rust Turso engine.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory, database, or schema cannot be
    /// prepared.
    pub async fn open_local(path: impl AsRef<Path>) -> Result<Self, TursoRepositoryError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| {
                TursoRepositoryError::PrepareDirectory {
                    path: parent.to_path_buf(),
                    source,
                }
            })?;
        }
        let path = path.to_string_lossy();
        let database = turso::Builder::new_local(&path).build().await?;
        let connection = database.connect()?;
        connection.execute("PRAGMA foreign_keys = ON", ()).await?;
        migrate(&connection).await?;
        Ok(Self { connection })
    }

    /// Returns the hidden local tenant, creating it on first launch.
    ///
    /// # Errors
    ///
    /// Returns an error when the tenant cannot be read or written.
    pub async fn ensure_local_tenant(
        &self,
        now: OffsetDateTime,
    ) -> Result<TenantId, TursoRepositoryError> {
        let mut rows = self
            .connection
            .query(
                "SELECT id FROM tenants WHERE slug = 'local' AND deleted_at IS NULL LIMIT 1",
                (),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            return parse_id("tenants.id", row.get(0)?);
        }
        let tenant = Tenant::local(TenantId::new(), now);
        self.connection
            .execute(
                r#"
                INSERT INTO tenants (
                    id, slug, name, status, created_at, updated_at, deleted_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
                params_from_iter(vec![
                    Value::from(tenant.id.to_string()),
                    Value::from(tenant.slug),
                    Value::from(tenant.name),
                    Value::from(encode_enum(&tenant.status)?),
                    Value::from(format_timestamp(tenant.timestamps.created_at)?),
                    Value::from(format_timestamp(tenant.timestamps.updated_at)?),
                    Value::Null,
                ]),
            )
            .await?;
        Ok(tenant.id)
    }
}

async fn migrate(connection: &Connection) -> Result<(), TursoRepositoryError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS _pah_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                applied_at TEXT NOT NULL
            );",
        )
        .await?;
    for (version, sql) in [(1_i64, INITIAL_SCHEMA), (2_i64, ASSET_FILES_SCHEMA)] {
        let mut rows = connection
            .query(
                "SELECT EXISTS(SELECT 1 FROM _pah_migrations WHERE version = ?)",
                [version],
            )
            .await?;
        let applied = rows
            .next()
            .await?
            .is_some_and(|row| row.get::<i64>(0).is_ok_and(|value| value != 0));
        if !applied {
            connection.execute_batch(sql).await?;
            connection
                .execute(
                    "INSERT INTO _pah_migrations (version, applied_at) VALUES (?, ?)",
                    (version, format_timestamp(OffsetDateTime::now_utc())?),
                )
                .await?;
        }
    }
    Ok(())
}

#[async_trait]
impl LibraryRepository for TursoLibraryRepository {
    async fn create_project(
        &self,
        tenant_id: TenantId,
        project: Project,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, project.tenant_id).map_err(repository_error)?;
        execute(
            &self.connection,
            r#"
            INSERT INTO projects (
                id, tenant_id, name, description, cover_asset_id, status,
                created_at, updated_at, deleted_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            vec![
                text(project.id),
                text(project.tenant_id),
                text(project.name),
                optional_text(project.description),
                optional_text(project.cover_asset_id.map(|id| id.to_string())),
                encoded(&project.status)?,
                timestamp(project.timestamps.created_at)?,
                timestamp(project.timestamps.updated_at)?,
                optional_timestamp(project.timestamps.deleted_at)?,
            ],
        )
        .await
    }

    async fn project_exists(
        &self,
        tenant_id: TenantId,
        project_id: ProjectId,
    ) -> Result<bool, RepositoryError> {
        exists(
            &self.connection,
            "projects",
            tenant_id,
            project_id.to_string(),
        )
        .await
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
        let mut connection = self.connection.clone();
        let transaction = connection.transaction().await.map_err(database_error)?;
        insert_prompt(&transaction, &prompt).await?;
        if let Some(variant) = initial_variant {
            insert_prompt_variant(&transaction, &variant).await?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn prompt_exists(
        &self,
        tenant_id: TenantId,
        prompt_id: PromptId,
    ) -> Result<bool, RepositoryError> {
        exists(
            &self.connection,
            "prompts",
            tenant_id,
            prompt_id.to_string(),
        )
        .await
    }

    async fn add_prompt_variant(
        &self,
        tenant_id: TenantId,
        variant: PromptVariant,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, variant.tenant_id).map_err(repository_error)?;
        insert_prompt_variant(&self.connection, &variant).await
    }

    async fn get_prompt_variant(
        &self,
        tenant_id: TenantId,
        variant_id: PromptVariantId,
    ) -> Result<Option<PromptVariant>, RepositoryError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, tenant_id, prompt_id, provider, model, model_version,
                       prompt_text, parameters_json, adaptation_notes, success_tips,
                       failure_patterns, status, revision, created_at, updated_at, deleted_at
                FROM prompt_variants
                WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
                "#,
                (tenant_id.to_string(), variant_id.to_string()),
            )
            .await
            .map_err(database_error)?;
        rows.next()
            .await
            .map_err(database_error)?
            .map(|row| prompt_variant_from_row(&row).map_err(repository_error))
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
        let mut connection = self.connection.clone();
        let transaction = connection.transaction().await.map_err(database_error)?;
        insert_asset(&transaction, &asset).await?;
        for file in &files {
            insert_asset_file(&transaction, file).await?;
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
        let placeholders = std::iter::repeat_n("?", asset_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id FROM assets WHERE tenant_id = ? AND deleted_at IS NULL AND id IN ({placeholders})"
        );
        let mut params = vec![text(tenant_id)];
        params.extend(asset_ids.iter().map(text));
        let mut rows = self
            .connection
            .query(sql, params_from_iter(params))
            .await
            .map_err(database_error)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            found.push(
                parse_id("assets.id", row.get(0).map_err(database_error)?)
                    .map_err(repository_error)?,
            );
        }
        Ok(found)
    }

    async fn record_generation(
        &self,
        tenant_id: TenantId,
        generation_run: GenerationRun,
        asset_links: Vec<AssetLink>,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, generation_run.tenant_id).map_err(repository_error)?;
        let mut connection = self.connection.clone();
        let transaction = connection.transaction().await.map_err(database_error)?;
        insert_generation_run(&transaction, &generation_run).await?;
        for link in &asset_links {
            ensure_tenant(tenant_id, link.tenant_id).map_err(repository_error)?;
            insert_asset_link(&transaction, link).await?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn create_evaluation(
        &self,
        tenant_id: TenantId,
        evaluation: Evaluation,
    ) -> Result<(), RepositoryError> {
        ensure_tenant(tenant_id, evaluation.tenant_id).map_err(repository_error)?;
        insert_evaluation(&self.connection, &evaluation).await
    }

    async fn replace_asset_tags(
        &self,
        tenant_id: TenantId,
        asset_id: AssetId,
        tags: Vec<String>,
        updated_at: OffsetDateTime,
    ) -> Result<(), RepositoryError> {
        let timestamp_value = timestamp(updated_at)?;
        let mut connection = self.connection.clone();
        let transaction = connection.transaction().await.map_err(database_error)?;
        let mut asset_rows = transaction
            .query(
                "SELECT 1 FROM assets WHERE tenant_id = ? AND id = ? \
                 AND deleted_at IS NULL LIMIT 1",
                (tenant_id.to_string(), asset_id.to_string()),
            )
            .await
            .map_err(database_error)?;
        if asset_rows.next().await.map_err(database_error)?.is_none() {
            return Err(RepositoryError::Other(
                "cannot tag an asset outside the requested tenant".into(),
            ));
        }
        execute(
            &transaction,
            "DELETE FROM asset_tags WHERE tenant_id = ? AND asset_id = ?",
            vec![text(tenant_id), text(asset_id)],
        )
        .await?;
        for tag in tags {
            let mut rows = transaction
                .query(
                    "SELECT id FROM tags WHERE tenant_id = ? AND category = 'custom' \
                     AND LOWER(name) = LOWER(?) AND deleted_at IS NULL LIMIT 1",
                    (tenant_id.to_string(), tag.clone()),
                )
                .await
                .map_err(database_error)?;
            let tag_id = rows
                .next()
                .await
                .map_err(database_error)?
                .map(|row| row.get::<String>(0))
                .transpose()
                .map_err(database_error)?
                .unwrap_or_else(|| TagId::new().to_string());
            execute(
                &transaction,
                "INSERT OR IGNORE INTO tags \
                 (id, tenant_id, name, category, parent_id, created_at, updated_at, deleted_at) \
                 VALUES (?, ?, ?, 'custom', NULL, ?, ?, NULL)",
                vec![
                    Value::Text(tag_id.clone()),
                    text(tenant_id),
                    Value::Text(tag),
                    timestamp_value.clone(),
                    timestamp_value.clone(),
                ],
            )
            .await?;
            execute(
                &transaction,
                "INSERT INTO asset_tags (tenant_id, asset_id, tag_id, created_at) \
                 VALUES (?, ?, ?, ?)",
                vec![
                    text(tenant_id),
                    text(asset_id),
                    Value::Text(tag_id),
                    timestamp_value.clone(),
                ],
            )
            .await?;
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

        let timestamp = timestamp(replacement.replaced_at)?;
        let mut connection = self.connection.clone();
        let transaction = connection.transaction().await.map_err(database_error)?;
        execute(
            &transaction,
            "UPDATE asset_links SET deleted_at = ?, updated_at = ? \
             WHERE tenant_id = ? AND asset_id = ? AND generation_run_id IS NOT NULL \
               AND deleted_at IS NULL",
            vec![
                timestamp.clone(),
                timestamp.clone(),
                text(tenant_id),
                text(replacement.asset_id),
            ],
        )
        .await?;
        execute(
            &transaction,
            "UPDATE evaluations SET deleted_at = ?, updated_at = ? \
             WHERE tenant_id = ? AND asset_id = ? AND deleted_at IS NULL",
            vec![
                timestamp.clone(),
                timestamp,
                text(tenant_id),
                text(replacement.asset_id),
            ],
        )
        .await?;
        if let Some(run) = &replacement.generation_run {
            insert_generation_run(&transaction, run).await?;
        }
        if let Some(link) = &replacement.asset_link {
            insert_asset_link(&transaction, link).await?;
        }
        if let Some(evaluation) = &replacement.evaluation {
            insert_evaluation(&transaction, evaluation).await?;
        }
        transaction.commit().await.map_err(database_error)
    }

    async fn search_library(
        &self,
        tenant_id: TenantId,
        criteria: SearchCriteria,
    ) -> Result<SearchPage<SearchRecord>, RepositoryError> {
        search_library(&self.connection, tenant_id, &criteria).await
    }

    async fn get_library_overview(
        &self,
        tenant_id: TenantId,
    ) -> Result<LibraryOverviewSnapshot, RepositoryError> {
        get_library_overview(&self.connection, tenant_id).await
    }

    async fn get_prompt_detail(
        &self,
        _tenant_id: TenantId,
        _prompt_id: PromptId,
    ) -> Result<Option<PromptDetailSnapshot>, RepositoryError> {
        Err(RepositoryError::Unsupported(
            "Turso detail projection is not enabled in the first adapter slice".into(),
        ))
    }

    async fn get_asset_detail(
        &self,
        tenant_id: TenantId,
        asset_id: AssetId,
    ) -> Result<Option<AssetDetailSnapshot>, RepositoryError> {
        get_asset_detail(&self.connection, tenant_id, asset_id).await
    }
}

async fn insert_prompt(connection: &Connection, prompt: &Prompt) -> Result<(), RepositoryError> {
    execute(
        connection,
        r#"
        INSERT INTO prompts (
            id, tenant_id, project_id, title, creative_intent, base_prompt,
            negative_constraints, status, current_version, created_by,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            text(prompt.id),
            text(prompt.tenant_id),
            text(prompt.project_id),
            text(prompt.title.clone()),
            text(prompt.creative_intent.clone()),
            text(prompt.base_prompt.clone()),
            optional_text(prompt.negative_constraints.clone()),
            encoded(&prompt.status)?,
            integer(prompt.current_version),
            text(prompt.created_by.clone()),
            timestamp(prompt.timestamps.created_at)?,
            timestamp(prompt.timestamps.updated_at)?,
            optional_timestamp(prompt.timestamps.deleted_at)?,
        ],
    )
    .await
}

async fn insert_prompt_variant(
    connection: &Connection,
    variant: &PromptVariant,
) -> Result<(), RepositoryError> {
    execute(
        connection,
        r#"
        INSERT INTO prompt_variants (
            id, tenant_id, prompt_id, provider, model, model_version,
            prompt_text, parameters_json, adaptation_notes, success_tips,
            failure_patterns, status, revision, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            text(variant.id),
            text(variant.tenant_id),
            text(variant.prompt_id),
            text(variant.provider.clone()),
            text(variant.model.clone()),
            optional_text(variant.model_version.clone()),
            text(variant.prompt_text.clone()),
            text(
                serde_json::to_string(&variant.parameters)
                    .map_err(TursoRepositoryError::from)
                    .map_err(repository_error)?,
            ),
            optional_text(variant.adaptation_notes.clone()),
            optional_text(variant.success_tips.clone()),
            optional_text(variant.failure_patterns.clone()),
            encoded(&variant.status)?,
            integer(variant.revision),
            timestamp(variant.timestamps.created_at)?,
            timestamp(variant.timestamps.updated_at)?,
            optional_timestamp(variant.timestamps.deleted_at)?,
        ],
    )
    .await
}

async fn insert_asset(connection: &Connection, asset: &Asset) -> Result<(), RepositoryError> {
    let (storage_key, external_url) = match &asset.location {
        AssetLocation::Stored { storage_key } => (Some(storage_key.clone()), None),
        AssetLocation::External { url } => (None, Some(url.clone())),
    };
    execute(
        connection,
        r#"
        INSERT INTO assets (
            id, tenant_id, media_type, storage_key, external_url, original_filename,
            mime_type, byte_size, width, height, duration_ms, sha256, thumbnail_key,
            visibility, status, metadata_json, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', ?, ?, ?)
        "#,
        vec![
            text(asset.id),
            text(asset.tenant_id),
            encoded(&asset.media_type)?,
            optional_text(storage_key),
            optional_text(external_url),
            optional_text(asset.original_filename.clone()),
            optional_text(asset.mime_type.clone()),
            optional_integer(asset.byte_size)?,
            optional_integer(asset.width.map(u64::from))?,
            optional_integer(asset.height.map(u64::from))?,
            optional_integer(asset.duration_ms)?,
            optional_text(asset.sha256.clone()),
            optional_text(asset.thumbnail_key.clone()),
            encoded(&asset.visibility)?,
            encoded(&asset.status)?,
            timestamp(asset.timestamps.created_at)?,
            timestamp(asset.timestamps.updated_at)?,
            optional_timestamp(asset.timestamps.deleted_at)?,
        ],
    )
    .await
}

async fn insert_asset_file(
    connection: &Connection,
    file: &AssetFile,
) -> Result<(), RepositoryError> {
    execute(
        connection,
        r#"
        INSERT INTO asset_files (
            id, tenant_id, asset_id, role, storage_key, mime_type, byte_size,
            width, height, sha256, generator_version, created_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
        vec![
            text(file.id),
            text(file.tenant_id),
            text(file.asset_id),
            encoded(&file.role)?,
            text(file.storage_key.clone()),
            text(file.mime_type.clone()),
            integer(file.byte_size),
            optional_integer(file.width.map(u64::from))?,
            optional_integer(file.height.map(u64::from))?,
            text(file.sha256.clone()),
            optional_text(file.generator_version.clone()),
            timestamp(file.created_at)?,
        ],
    )
    .await
}

async fn insert_generation_run(
    connection: &Connection,
    run: &GenerationRun,
) -> Result<(), RepositoryError> {
    let (amount, currency) = run.cost.as_ref().map_or((None, None), |cost| {
        (Some(cost.amount_micros), Some(cost.currency.clone()))
    });
    execute(
        connection,
        r#"
        INSERT INTO generation_runs (
            id, tenant_id, variant_id, source, input_snapshot_json,
            parameters_snapshot_json, external_url, started_at, completed_at,
            status, failure_message, cost_amount_micros, cost_currency, created_by,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            text(run.id),
            text(run.tenant_id),
            text(run.variant_id),
            encoded(&run.source)?,
            text(
                serde_json::to_string(&run.input_snapshot)
                    .map_err(TursoRepositoryError::from)
                    .map_err(repository_error)?,
            ),
            text(
                serde_json::to_string(&run.parameters_snapshot)
                    .map_err(TursoRepositoryError::from)
                    .map_err(repository_error)?,
            ),
            optional_text(run.external_url.clone()),
            optional_timestamp(run.started_at)?,
            optional_timestamp(run.completed_at)?,
            encoded(&run.status)?,
            optional_text(run.failure_message.clone()),
            optional_integer(amount)?,
            optional_text(currency),
            text(run.created_by.clone()),
            timestamp(run.timestamps.created_at)?,
            timestamp(run.timestamps.updated_at)?,
            optional_timestamp(run.timestamps.deleted_at)?,
        ],
    )
    .await
}

async fn insert_asset_link(
    connection: &Connection,
    link: &AssetLink,
) -> Result<(), RepositoryError> {
    let created_at = timestamp(link.created_at)?;
    execute(
        connection,
        r#"
        INSERT INTO asset_links (
            id, tenant_id, asset_id, generation_run_id, prompt_id, related_asset_id,
            relation_type, sort_order, note, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
        vec![
            text(link.id),
            text(link.tenant_id),
            text(link.asset_id),
            optional_text(link.generation_run_id.map(|id| id.to_string())),
            optional_text(link.prompt_id.map(|id| id.to_string())),
            optional_text(link.related_asset_id.map(|id| id.to_string())),
            encoded(&link.relation_type)?,
            integer(link.sort_order),
            optional_text(link.note.clone()),
            created_at.clone(),
            created_at,
        ],
    )
    .await
}

async fn insert_evaluation(
    connection: &Connection,
    evaluation: &Evaluation,
) -> Result<(), RepositoryError> {
    execute(
        connection,
        r#"
        INSERT INTO evaluations (
            id, tenant_id, asset_id, generation_run_id, overall_score,
            is_featured, is_reusable, dimension_scores_json, strengths, issues,
            lesson, created_by, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            text(evaluation.id),
            text(evaluation.tenant_id),
            text(evaluation.asset_id),
            optional_text(evaluation.generation_run_id.map(|id| id.to_string())),
            integer(evaluation.overall_score),
            Value::from(evaluation.is_featured),
            Value::from(evaluation.is_reusable),
            text(
                serde_json::to_string(&evaluation.dimension_scores)
                    .map_err(TursoRepositoryError::from)
                    .map_err(repository_error)?,
            ),
            optional_text(evaluation.strengths.clone()),
            optional_text(evaluation.issues.clone()),
            optional_text(evaluation.lesson.clone()),
            text(evaluation.created_by.clone()),
            timestamp(evaluation.timestamps.created_at)?,
            timestamp(evaluation.timestamps.updated_at)?,
            optional_timestamp(evaluation.timestamps.deleted_at)?,
        ],
    )
    .await
}

async fn search_library(
    connection: &Connection,
    tenant_id: TenantId,
    criteria: &SearchCriteria,
) -> Result<SearchPage<SearchRecord>, RepositoryError> {
    let wants_assets = criteria.kinds.is_empty() || criteria.kinds.contains(&SearchKind::Asset);
    let wants_prompts = criteria.kinds.is_empty() || criteria.kinds.contains(&SearchKind::Prompt);
    let mut records = Vec::new();
    if wants_assets {
        let mut sql = String::from(
            r#"
            SELECT a.id, a.original_filename, a.media_type, a.thumbnail_key, a.created_at,
                   (SELECT MAX(e.overall_score) FROM evaluations e
                    WHERE e.tenant_id = a.tenant_id AND e.asset_id = a.id AND e.deleted_at IS NULL),
                   EXISTS(SELECT 1 FROM evaluations e
                    WHERE e.tenant_id = a.tenant_id AND e.asset_id = a.id
                      AND e.is_featured = 1 AND e.deleted_at IS NULL),
                   (SELECT p.title
                      FROM asset_links al
                      JOIN prompts p ON p.tenant_id = al.tenant_id AND p.id = al.prompt_id
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL AND p.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1),
                   (SELECT p.project_id
                      FROM asset_links al
                      JOIN prompts p ON p.tenant_id = al.tenant_id AND p.id = al.prompt_id
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL AND p.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1),
                   (SELECT project.name
                      FROM asset_links al
                      JOIN prompts p ON p.tenant_id = al.tenant_id AND p.id = al.prompt_id
                      JOIN projects project
                        ON project.tenant_id = p.tenant_id AND project.id = p.project_id
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL AND p.deleted_at IS NULL
                       AND project.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1),
                   (SELECT pv.provider
                      FROM asset_links al
                      JOIN generation_runs gr
                        ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
                      JOIN prompt_variants pv
                        ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL AND gr.deleted_at IS NULL
                       AND pv.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1),
                   (SELECT pv.model
                      FROM asset_links al
                      JOIN generation_runs gr
                        ON gr.tenant_id = al.tenant_id AND gr.id = al.generation_run_id
                      JOIN prompt_variants pv
                        ON pv.tenant_id = gr.tenant_id AND pv.id = gr.variant_id
                     WHERE al.tenant_id = a.tenant_id AND al.asset_id = a.id
                       AND al.deleted_at IS NULL AND gr.deleted_at IS NULL
                       AND pv.deleted_at IS NULL
                     ORDER BY al.sort_order ASC, al.id ASC LIMIT 1),
                   COALESCE((
                     SELECT json_group_array(t.name)
                       FROM asset_tags atag
                       JOIN tags t ON t.tenant_id = atag.tenant_id AND t.id = atag.tag_id
                      WHERE atag.tenant_id = a.tenant_id AND atag.asset_id = a.id
                        AND t.deleted_at IS NULL
                   ), '[]')
            FROM assets a
            WHERE a.tenant_id = ? AND a.deleted_at IS NULL
            "#,
        );
        let mut params = vec![text(tenant_id)];
        if !criteria.include_archived {
            sql.push_str(" AND a.status != 'archived'");
        }
        if let Some(query) = criteria
            .query
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            sql.push_str(" AND LOWER(COALESCE(a.original_filename, '')) LIKE ?");
            params.push(text(format!("%{}%", query.trim().to_lowercase())));
        }
        if !criteria.media_types.is_empty() {
            push_in_clause(&mut sql, "a.media_type", criteria.media_types.len());
            for media_type in &criteria.media_types {
                params.push(encoded(media_type)?);
            }
        }
        for tag in &criteria.tags {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM asset_tags atag \
                 JOIN tags t ON t.tenant_id = atag.tenant_id AND t.id = atag.tag_id \
                 WHERE atag.tenant_id = a.tenant_id AND atag.asset_id = a.id \
                   AND t.deleted_at IS NULL AND t.name = ?)",
            );
            params.push(Value::Text(tag.trim().to_owned()));
        }
        sql.push_str(" ORDER BY a.created_at DESC LIMIT ?");
        params.push(integer(criteria.limit.saturating_add(1)));
        let mut rows = connection
            .query(sql, params_from_iter(params))
            .await
            .map_err(database_error)?;
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let media_type: String = row.get(2).map_err(database_error)?;
            let score: Option<i64> = row.get(5).map_err(database_error)?;
            let featured: i64 = row.get(6).map_err(database_error)?;
            if criteria
                .minimum_score
                .is_some_and(|minimum| score.unwrap_or_default() < i64::from(minimum))
                || (criteria.featured_only && featured == 0)
            {
                continue;
            }
            let overall_score = score
                .map(u8::try_from)
                .transpose()
                .map_err(|_| RepositoryError::CorruptData("evaluation score is invalid".into()))?;
            records.push(SearchRecord {
                tenant_id,
                id: SearchRecordId::Asset(
                    parse_id("assets.id", row.get(0).map_err(database_error)?)
                        .map_err(repository_error)?,
                ),
                title: row
                    .get::<Option<String>>(1)
                    .map_err(database_error)?
                    .unwrap_or_else(|| "Untitled asset".into()),
                summary: row.get(7).map_err(database_error)?,
                project_id: row
                    .get::<Option<String>>(8)
                    .map_err(database_error)?
                    .map(|value| parse_id("assets.project_id", value))
                    .transpose()
                    .map_err(repository_error)?,
                project_name: row.get(9).map_err(database_error)?,
                media_type: Some(
                    decode_enum("assets.media_type", media_type).map_err(repository_error)?,
                ),
                provider: row.get(10).map_err(database_error)?,
                model: row.get(11).map_err(database_error)?,
                thumbnail_key: row.get(3).map_err(database_error)?,
                overall_score,
                is_featured: featured != 0,
                tags: serde_json::from_str(&row.get::<String>(12).map_err(database_error)?)
                    .map_err(TursoRepositoryError::from)
                    .map_err(repository_error)?,
                created_at: parse_timestamp(
                    "assets.created_at",
                    row.get(4).map_err(database_error)?,
                )
                .map_err(repository_error)?,
            });
        }
    }
    if wants_prompts && criteria.media_types.is_empty() {
        let mut sql = String::from(
            r#"
            SELECT p.id, p.title, p.creative_intent, p.project_id, pr.name, p.created_at,
                   COALESCE((
                     SELECT json_group_array(t.name)
                       FROM prompt_tags pt
                       JOIN tags t ON t.tenant_id = pt.tenant_id AND t.id = pt.tag_id
                      WHERE pt.tenant_id = p.tenant_id AND pt.prompt_id = p.id
                        AND t.deleted_at IS NULL
                   ), '[]')
            FROM prompts p
            JOIN projects pr ON pr.tenant_id = p.tenant_id AND pr.id = p.project_id
            WHERE p.tenant_id = ? AND p.deleted_at IS NULL
            "#,
        );
        let mut params = vec![text(tenant_id)];
        if !criteria.include_archived {
            sql.push_str(" AND p.status != 'archived'");
        }
        if let Some(query) = criteria
            .query
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            sql.push_str(
                " AND (LOWER(p.title) LIKE ? OR LOWER(p.base_prompt) LIKE ? OR LOWER(p.creative_intent) LIKE ?)",
            );
            let like = text(format!("%{}%", query.trim().to_lowercase()));
            params.extend([like.clone(), like.clone(), like]);
        }
        for tag in &criteria.tags {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM prompt_tags pt \
                 JOIN tags t ON t.tenant_id = pt.tenant_id AND t.id = pt.tag_id \
                 WHERE pt.tenant_id = p.tenant_id AND pt.prompt_id = p.id \
                   AND t.deleted_at IS NULL AND t.name = ?)",
            );
            params.push(Value::Text(tag.trim().to_owned()));
        }
        sql.push_str(" ORDER BY p.created_at DESC LIMIT ?");
        params.push(integer(criteria.limit.saturating_add(1)));
        let mut rows = connection
            .query(sql, params_from_iter(params))
            .await
            .map_err(database_error)?;
        while let Some(row) = rows.next().await.map_err(database_error)? {
            records.push(SearchRecord {
                tenant_id,
                id: SearchRecordId::Prompt(
                    parse_id("prompts.id", row.get(0).map_err(database_error)?)
                        .map_err(repository_error)?,
                ),
                title: row.get(1).map_err(database_error)?,
                summary: row.get(2).map_err(database_error)?,
                project_id: Some(
                    parse_id("prompts.project_id", row.get(3).map_err(database_error)?)
                        .map_err(repository_error)?,
                ),
                project_name: row.get(4).map_err(database_error)?,
                media_type: None,
                provider: None,
                model: None,
                thumbnail_key: None,
                overall_score: None,
                is_featured: false,
                tags: serde_json::from_str(&row.get::<String>(6).map_err(database_error)?)
                    .map_err(TursoRepositoryError::from)
                    .map_err(repository_error)?,
                created_at: parse_timestamp(
                    "prompts.created_at",
                    row.get(5).map_err(database_error)?,
                )
                .map_err(repository_error)?,
            });
        }
    }
    records.sort_by_key(|record| Reverse(record.created_at));
    records.truncate(criteria.limit as usize);
    Ok(SearchPage {
        items: records,
        next_cursor: None,
    })
}

async fn exists(
    connection: &Connection,
    table: &'static str,
    tenant_id: TenantId,
    id: String,
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
    let mut rows = connection
        .query(sql, (tenant_id.to_string(), id))
        .await
        .map_err(database_error)?;
    Ok(rows
        .next()
        .await
        .map_err(database_error)?
        .is_some_and(|row| row.get::<i64>(0).is_ok_and(|value| value != 0)))
}

async fn get_asset_detail(
    connection: &Connection,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Option<AssetDetailSnapshot>, RepositoryError> {
    let Some(asset) = load_asset(connection, tenant_id, asset_id).await? else {
        return Ok(None);
    };
    let links = load_asset_links(connection, tenant_id, asset_id).await?;
    let mut generation_runs = Vec::new();
    let mut variants = Vec::new();
    let mut prompts = Vec::new();
    let mut seen_runs = BTreeSet::new();
    let mut seen_variants = BTreeSet::new();
    let mut seen_prompts = BTreeSet::new();

    for link in &links {
        if let Some(run_id) = link.generation_run_id
            && seen_runs.insert(run_id)
            && let Some(run) = load_generation_run(connection, tenant_id, run_id).await?
        {
            let variant_id = run.variant_id;
            generation_runs.push(run);
            if seen_variants.insert(variant_id)
                && let Some(variant) =
                    load_prompt_variant(connection, tenant_id, variant_id).await?
            {
                let prompt_id = variant.prompt_id;
                variants.push(variant);
                if seen_prompts.insert(prompt_id)
                    && let Some(prompt) = load_prompt(connection, tenant_id, prompt_id).await?
                {
                    prompts.push(prompt);
                }
            }
        }
        if let Some(prompt_id) = link.prompt_id
            && seen_prompts.insert(prompt_id)
            && let Some(prompt) = load_prompt(connection, tenant_id, prompt_id).await?
        {
            prompts.push(prompt);
        }
    }

    let evaluations = load_evaluations(connection, tenant_id, asset_id).await?;
    let related_assets = load_related_assets(connection, tenant_id, asset_id).await?;
    let tags = load_asset_tags(connection, tenant_id, asset_id).await?;
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

async fn load_asset_tags(
    connection: &Connection,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Vec<String>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT t.name
              FROM asset_tags atag
              JOIN tags t ON t.tenant_id = atag.tenant_id AND t.id = atag.tag_id
             WHERE atag.tenant_id = ? AND atag.asset_id = ? AND t.deleted_at IS NULL
             ORDER BY LOWER(t.name) ASC, t.id ASC
            "#,
            (tenant_id.to_string(), asset_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    let mut tags = Vec::new();
    while let Some(row) = rows.next().await.map_err(database_error)? {
        tags.push(row.get(0).map_err(database_error)?);
    }
    Ok(tags)
}

async fn load_asset(
    connection: &Connection,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Option<Asset>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT id, tenant_id, media_type, storage_key, external_url,
                   original_filename, mime_type, byte_size, width, height,
                   duration_ms, sha256, thumbnail_key, visibility, status,
                   created_at, updated_at, deleted_at
              FROM assets
             WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
            "#,
            (tenant_id.to_string(), asset_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    rows.next()
        .await
        .map_err(database_error)?
        .map(|row| asset_from_row(&row).map_err(repository_error))
        .transpose()
}

async fn load_asset_links(
    connection: &Connection,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Vec<AssetLink>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT id, tenant_id, asset_id, generation_run_id, prompt_id,
                   related_asset_id, relation_type, sort_order, note, created_at
              FROM asset_links
             WHERE tenant_id = ? AND asset_id = ? AND deleted_at IS NULL
             ORDER BY sort_order ASC, created_at ASC, id ASC
            "#,
            (tenant_id.to_string(), asset_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    let mut links = Vec::new();
    while let Some(row) = rows.next().await.map_err(database_error)? {
        links.push(asset_link_from_row(&row).map_err(repository_error)?);
    }
    Ok(links)
}

async fn load_generation_run(
    connection: &Connection,
    tenant_id: TenantId,
    run_id: pah_domain::GenerationRunId,
) -> Result<Option<GenerationRun>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT id, tenant_id, variant_id, source, input_snapshot_json,
                   parameters_snapshot_json, external_url, started_at, completed_at,
                   status, failure_message, cost_amount_micros, cost_currency,
                   created_by, created_at, updated_at, deleted_at
              FROM generation_runs
             WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
            "#,
            (tenant_id.to_string(), run_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    rows.next()
        .await
        .map_err(database_error)?
        .map(|row| generation_run_from_row(&row).map_err(repository_error))
        .transpose()
}

async fn load_prompt_variant(
    connection: &Connection,
    tenant_id: TenantId,
    variant_id: PromptVariantId,
) -> Result<Option<PromptVariant>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT id, tenant_id, prompt_id, provider, model, model_version,
                   prompt_text, parameters_json, adaptation_notes, success_tips,
                   failure_patterns, status, revision, created_at, updated_at, deleted_at
              FROM prompt_variants
             WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
            "#,
            (tenant_id.to_string(), variant_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    rows.next()
        .await
        .map_err(database_error)?
        .map(|row| prompt_variant_from_row(&row).map_err(repository_error))
        .transpose()
}

async fn load_prompt(
    connection: &Connection,
    tenant_id: TenantId,
    prompt_id: PromptId,
) -> Result<Option<Prompt>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT id, tenant_id, project_id, title, creative_intent, base_prompt,
                   negative_constraints, status, current_version, created_by,
                   created_at, updated_at, deleted_at
              FROM prompts
             WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL
            "#,
            (tenant_id.to_string(), prompt_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    rows.next()
        .await
        .map_err(database_error)?
        .map(|row| prompt_from_row(&row).map_err(repository_error))
        .transpose()
}

async fn load_evaluations(
    connection: &Connection,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Vec<Evaluation>, RepositoryError> {
    let mut rows = connection
        .query(
            r#"
            SELECT id, tenant_id, asset_id, generation_run_id, overall_score,
                   is_featured, is_reusable, dimension_scores_json, strengths,
                   issues, lesson, created_by, created_at, updated_at, deleted_at
              FROM evaluations
             WHERE tenant_id = ? AND asset_id = ? AND deleted_at IS NULL
             ORDER BY created_at DESC, id ASC
            "#,
            (tenant_id.to_string(), asset_id.to_string()),
        )
        .await
        .map_err(database_error)?;
    let mut evaluations = Vec::new();
    while let Some(row) = rows.next().await.map_err(database_error)? {
        evaluations.push(evaluation_from_row(&row).map_err(repository_error)?);
    }
    Ok(evaluations)
}

async fn load_related_assets(
    connection: &Connection,
    tenant_id: TenantId,
    asset_id: AssetId,
) -> Result<Vec<Asset>, RepositoryError> {
    let mut rows = connection
        .query(
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
            (
                tenant_id.to_string(),
                asset_id.to_string(),
                asset_id.to_string(),
                asset_id.to_string(),
            ),
        )
        .await
        .map_err(database_error)?;
    let mut assets = Vec::new();
    while let Some(row) = rows.next().await.map_err(database_error)? {
        assets.push(asset_from_row(&row).map_err(repository_error)?);
    }
    Ok(assets)
}

async fn get_library_overview(
    connection: &Connection,
    tenant_id: TenantId,
) -> Result<LibraryOverviewSnapshot, RepositoryError> {
    let mut project_rows = connection
        .query(
            r#"
            SELECT id, tenant_id, name, description, cover_asset_id, status,
                   created_at, updated_at, deleted_at
              FROM projects
             WHERE tenant_id = ? AND deleted_at IS NULL
             ORDER BY updated_at DESC, id ASC
            "#,
            [tenant_id.to_string()],
        )
        .await
        .map_err(database_error)?;
    let mut projects = Vec::new();
    while let Some(row) = project_rows.next().await.map_err(database_error)? {
        projects.push(project_from_row(&row).map_err(repository_error)?);
    }

    let mut prompt_rows = connection
        .query(
            r#"
            SELECT id, tenant_id, project_id, title, creative_intent, base_prompt,
                   negative_constraints, status, current_version, created_by,
                   created_at, updated_at, deleted_at
              FROM prompts
             WHERE tenant_id = ? AND deleted_at IS NULL
             ORDER BY updated_at DESC, id ASC
            "#,
            [tenant_id.to_string()],
        )
        .await
        .map_err(database_error)?;
    let mut prompts = Vec::new();
    while let Some(row) = prompt_rows.next().await.map_err(database_error)? {
        prompts.push(prompt_from_row(&row).map_err(repository_error)?);
    }

    let mut variant_rows = connection
        .query(
            r#"
            SELECT id, tenant_id, prompt_id, provider, model, model_version,
                   prompt_text, parameters_json, adaptation_notes, success_tips,
                   failure_patterns, status, revision, created_at, updated_at, deleted_at
              FROM prompt_variants
             WHERE tenant_id = ? AND deleted_at IS NULL
             ORDER BY updated_at DESC, id ASC
            "#,
            [tenant_id.to_string()],
        )
        .await
        .map_err(database_error)?;
    let mut variants = Vec::new();
    while let Some(row) = variant_rows.next().await.map_err(database_error)? {
        variants.push(prompt_variant_from_row(&row).map_err(repository_error)?);
    }

    let mut count_rows = connection
        .query(
            r#"
            SELECT gr.variant_id, COUNT(DISTINCT al.asset_id)
              FROM generation_runs gr
              LEFT JOIN asset_links al
                ON al.tenant_id = gr.tenant_id AND al.generation_run_id = gr.id
               AND al.relation_type = 'output' AND al.deleted_at IS NULL
             WHERE gr.tenant_id = ? AND gr.deleted_at IS NULL
             GROUP BY gr.variant_id
            "#,
            [tenant_id.to_string()],
        )
        .await
        .map_err(database_error)?;
    let mut variant_result_counts = std::collections::BTreeMap::new();
    while let Some(row) = count_rows.next().await.map_err(database_error)? {
        variant_result_counts.insert(
            parse_id(
                "generation_runs.variant_id",
                row.get(0).map_err(database_error)?,
            )
            .map_err(repository_error)?,
            i64_to_u64(
                "generation_runs.result_count",
                row.get(1).map_err(database_error)?,
            )
            .map_err(repository_error)?,
        );
    }

    Ok(LibraryOverviewSnapshot {
        projects,
        prompts,
        variants,
        variant_result_counts,
    })
}

async fn execute(
    connection: &Connection,
    sql: &str,
    params: Vec<Value>,
) -> Result<(), RepositoryError> {
    connection
        .execute(sql, params_from_iter(params))
        .await
        .map(|_| ())
        .map_err(database_error)
}

fn push_in_clause(sql: &mut String, column: &str, count: usize) {
    let placeholders = std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ");
    let _ = write!(sql, " AND {column} IN ({placeholders})");
}

fn prompt_variant_from_row(row: &turso::Row) -> Result<PromptVariant, TursoRepositoryError> {
    Ok(PromptVariant {
        id: parse_id("prompt_variants.id", row.get(0)?)?,
        tenant_id: parse_id("prompt_variants.tenant_id", row.get(1)?)?,
        prompt_id: parse_id("prompt_variants.prompt_id", row.get(2)?)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        model_version: row.get(5)?,
        prompt_text: row.get(6)?,
        parameters: serde_json::from_str(&row.get::<String>(7)?)?,
        adaptation_notes: row.get(8)?,
        success_tips: row.get(9)?,
        failure_patterns: row.get(10)?,
        status: decode_enum("prompt_variants.status", row.get(11)?)?,
        revision: u32::try_from(row.get::<i64>(12)?).map_err(|_| {
            TursoRepositoryError::InvalidData {
                field: "prompt_variants.revision",
                value: "out of range".into(),
            }
        })?,
        timestamps: pah_domain::RecordTimestamps {
            created_at: parse_timestamp("prompt_variants.created_at", row.get(13)?)?,
            updated_at: parse_timestamp("prompt_variants.updated_at", row.get(14)?)?,
            deleted_at: row
                .get::<Option<String>>(15)?
                .map(|value| parse_timestamp("prompt_variants.deleted_at", value))
                .transpose()?,
        },
    })
}

fn project_from_row(row: &turso::Row) -> Result<Project, TursoRepositoryError> {
    Ok(Project {
        id: parse_id("projects.id", row.get(0)?)?,
        tenant_id: parse_id("projects.tenant_id", row.get(1)?)?,
        name: row.get(2)?,
        description: row.get(3)?,
        cover_asset_id: row
            .get::<Option<String>>(4)?
            .map(|value| parse_id("projects.cover_asset_id", value))
            .transpose()?,
        status: decode_enum("projects.status", row.get(5)?)?,
        timestamps: pah_domain::RecordTimestamps {
            created_at: parse_timestamp("projects.created_at", row.get(6)?)?,
            updated_at: parse_timestamp("projects.updated_at", row.get(7)?)?,
            deleted_at: row
                .get::<Option<String>>(8)?
                .map(|value| parse_timestamp("projects.deleted_at", value))
                .transpose()?,
        },
    })
}

fn prompt_from_row(row: &turso::Row) -> Result<Prompt, TursoRepositoryError> {
    Ok(Prompt {
        id: parse_id("prompts.id", row.get(0)?)?,
        tenant_id: parse_id("prompts.tenant_id", row.get(1)?)?,
        project_id: parse_id("prompts.project_id", row.get(2)?)?,
        title: row.get(3)?,
        creative_intent: row.get(4)?,
        base_prompt: row.get(5)?,
        negative_constraints: row.get(6)?,
        status: decode_enum("prompts.status", row.get(7)?)?,
        current_version: u32::try_from(row.get::<i64>(8)?).map_err(|_| {
            TursoRepositoryError::InvalidData {
                field: "prompts.current_version",
                value: "out of range".into(),
            }
        })?,
        created_by: row.get(9)?,
        timestamps: pah_domain::RecordTimestamps {
            created_at: parse_timestamp("prompts.created_at", row.get(10)?)?,
            updated_at: parse_timestamp("prompts.updated_at", row.get(11)?)?,
            deleted_at: row
                .get::<Option<String>>(12)?
                .map(|value| parse_timestamp("prompts.deleted_at", value))
                .transpose()?,
        },
    })
}

fn asset_from_row(row: &turso::Row) -> Result<Asset, TursoRepositoryError> {
    let storage_key = row.get::<Option<String>>(3)?;
    let external_url = row.get::<Option<String>>(4)?;
    let location = match (storage_key, external_url) {
        (Some(storage_key), None) => AssetLocation::Stored { storage_key },
        (None, Some(url)) => AssetLocation::External { url },
        (storage_key, external_url) => {
            return Err(TursoRepositoryError::InvalidData {
                field: "assets.location",
                value: format!("storage_key={storage_key:?}, external_url={external_url:?}"),
            });
        }
    };
    Ok(Asset {
        id: parse_id("assets.id", row.get(0)?)?,
        tenant_id: parse_id("assets.tenant_id", row.get(1)?)?,
        media_type: decode_enum("assets.media_type", row.get(2)?)?,
        location,
        original_filename: row.get(5)?,
        mime_type: row.get(6)?,
        byte_size: optional_i64_to_u64("assets.byte_size", row.get(7)?)?,
        width: optional_i64_to_u32("assets.width", row.get(8)?)?,
        height: optional_i64_to_u32("assets.height", row.get(9)?)?,
        duration_ms: optional_i64_to_u64("assets.duration_ms", row.get(10)?)?,
        sha256: row.get(11)?,
        thumbnail_key: row.get(12)?,
        visibility: decode_enum("assets.visibility", row.get(13)?)?,
        status: decode_enum("assets.status", row.get(14)?)?,
        timestamps: record_timestamps(row, 15, "assets")?,
    })
}

fn asset_link_from_row(row: &turso::Row) -> Result<AssetLink, TursoRepositoryError> {
    Ok(AssetLink {
        id: parse_id("asset_links.id", row.get(0)?)?,
        tenant_id: parse_id("asset_links.tenant_id", row.get(1)?)?,
        asset_id: parse_id("asset_links.asset_id", row.get(2)?)?,
        generation_run_id: parse_optional_id("asset_links.generation_run_id", row.get(3)?)?,
        prompt_id: parse_optional_id("asset_links.prompt_id", row.get(4)?)?,
        related_asset_id: parse_optional_id("asset_links.related_asset_id", row.get(5)?)?,
        relation_type: decode_enum("asset_links.relation_type", row.get(6)?)?,
        sort_order: i64_to_u32("asset_links.sort_order", row.get(7)?)?,
        note: row.get(8)?,
        created_at: parse_timestamp("asset_links.created_at", row.get(9)?)?,
    })
}

fn generation_run_from_row(row: &turso::Row) -> Result<GenerationRun, TursoRepositoryError> {
    let amount = row.get::<Option<i64>>(11)?;
    let currency = row.get::<Option<String>>(12)?;
    let cost = match (amount, currency) {
        (Some(amount), Some(currency)) => Some(GenerationCost {
            amount_micros: i64_to_u64("generation_runs.cost_amount_micros", amount)?,
            currency,
        }),
        (None, None) => None,
        (amount, currency) => {
            return Err(TursoRepositoryError::InvalidData {
                field: "generation_runs.cost",
                value: format!("amount={amount:?}, currency={currency:?}"),
            });
        }
    };
    Ok(GenerationRun {
        id: parse_id("generation_runs.id", row.get(0)?)?,
        tenant_id: parse_id("generation_runs.tenant_id", row.get(1)?)?,
        variant_id: parse_id("generation_runs.variant_id", row.get(2)?)?,
        source: decode_enum("generation_runs.source", row.get(3)?)?,
        input_snapshot: serde_json::from_str(&row.get::<String>(4)?)?,
        parameters_snapshot: serde_json::from_str(&row.get::<String>(5)?)?,
        external_url: row.get(6)?,
        started_at: parse_optional_timestamp("generation_runs.started_at", row.get(7)?)?,
        completed_at: parse_optional_timestamp("generation_runs.completed_at", row.get(8)?)?,
        status: decode_enum("generation_runs.status", row.get(9)?)?,
        failure_message: row.get(10)?,
        cost,
        created_by: row.get(13)?,
        timestamps: record_timestamps(row, 14, "generation_runs")?,
    })
}

fn evaluation_from_row(row: &turso::Row) -> Result<Evaluation, TursoRepositoryError> {
    Ok(Evaluation {
        id: parse_id("evaluations.id", row.get(0)?)?,
        tenant_id: parse_id("evaluations.tenant_id", row.get(1)?)?,
        asset_id: parse_id("evaluations.asset_id", row.get(2)?)?,
        generation_run_id: parse_optional_id("evaluations.generation_run_id", row.get(3)?)?,
        overall_score: u8::try_from(row.get::<i64>(4)?).map_err(|_| {
            TursoRepositoryError::InvalidData {
                field: "evaluations.overall_score",
                value: "out of range".into(),
            }
        })?,
        is_featured: row.get::<i64>(5)? != 0,
        is_reusable: row.get::<i64>(6)? != 0,
        dimension_scores: serde_json::from_str(&row.get::<String>(7)?)?,
        strengths: row.get(8)?,
        issues: row.get(9)?,
        lesson: row.get(10)?,
        created_by: row.get(11)?,
        timestamps: record_timestamps(row, 12, "evaluations")?,
    })
}

fn record_timestamps(
    row: &turso::Row,
    start: usize,
    entity: &'static str,
) -> Result<pah_domain::RecordTimestamps, TursoRepositoryError> {
    Ok(pah_domain::RecordTimestamps {
        created_at: parse_timestamp("created_at", row.get(start)?)?,
        updated_at: parse_timestamp("updated_at", row.get(start + 1)?)?,
        deleted_at: row
            .get::<Option<String>>(start + 2)?
            .map(|value| parse_timestamp("deleted_at", value))
            .transpose()
            .map_err(|error| TursoRepositoryError::InvalidData {
                field: "timestamps",
                value: format!("{entity}: {error}"),
            })?,
    })
}

fn parse_optional_timestamp(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<OffsetDateTime>, TursoRepositoryError> {
    value.map(|value| parse_timestamp(field, value)).transpose()
}

fn parse_optional_id<T>(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<T>, TursoRepositoryError>
where
    T: std::str::FromStr,
{
    value.map(|value| parse_id(field, value)).transpose()
}

fn optional_i64_to_u32(
    field: &'static str,
    value: Option<i64>,
) -> Result<Option<u32>, TursoRepositoryError> {
    value.map(|value| i64_to_u32(field, value)).transpose()
}

fn optional_i64_to_u64(
    field: &'static str,
    value: Option<i64>,
) -> Result<Option<u64>, TursoRepositoryError> {
    value.map(|value| i64_to_u64(field, value)).transpose()
}

fn i64_to_u32(field: &'static str, value: i64) -> Result<u32, TursoRepositoryError> {
    u32::try_from(value).map_err(|_| TursoRepositoryError::InvalidData {
        field,
        value: value.to_string(),
    })
}

fn i64_to_u64(field: &'static str, value: i64) -> Result<u64, TursoRepositoryError> {
    u64::try_from(value).map_err(|_| TursoRepositoryError::InvalidData {
        field,
        value: value.to_string(),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn text(value: impl ToString) -> Value {
    Value::from(value.to_string())
}

fn optional_text(value: Option<String>) -> Value {
    Value::from(value)
}

fn integer(value: impl Into<u64>) -> Value {
    optional_integer(Some(value.into())).expect("domain integers fit in i64")
}

fn optional_integer(value: Option<u64>) -> Result<Value, RepositoryError> {
    value
        .map(|value| {
            i64::try_from(value)
                .map(Value::from)
                .map_err(|_| RepositoryError::CorruptData(format!("integer {value} is too large")))
        })
        .transpose()
        .map(|value| value.unwrap_or(Value::Null))
}

fn encoded<T: Serialize>(value: &T) -> Result<Value, RepositoryError> {
    encode_enum(value)
        .map(Value::from)
        .map_err(repository_error)
}

fn timestamp(value: OffsetDateTime) -> Result<Value, RepositoryError> {
    format_timestamp(value)
        .map(Value::from)
        .map_err(repository_error)
}

fn optional_timestamp(value: Option<OffsetDateTime>) -> Result<Value, RepositoryError> {
    value
        .map(format_timestamp)
        .transpose()
        .map(Value::from)
        .map_err(repository_error)
}

fn ensure_tenant(
    requested_tenant: TenantId,
    entity_tenant: TenantId,
) -> Result<(), TursoRepositoryError> {
    if requested_tenant == entity_tenant {
        Ok(())
    } else {
        Err(TursoRepositoryError::TenantMismatch {
            requested_tenant,
            entity_tenant,
        })
    }
}

fn encode_enum<T: Serialize>(value: &T) -> Result<String, TursoRepositoryError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        value => Err(TursoRepositoryError::InvalidData {
            field: "enum",
            value: value.to_string(),
        }),
    }
}

fn decode_enum<T: DeserializeOwned>(
    field: &'static str,
    value: String,
) -> Result<T, TursoRepositoryError> {
    serde_json::from_value(serde_json::Value::String(value.clone()))
        .map_err(|_| TursoRepositoryError::InvalidData { field, value })
}

fn format_timestamp(value: OffsetDateTime) -> Result<String, TursoRepositoryError> {
    let value = value.to_offset(UtcOffset::UTC);
    if !(0..=9999).contains(&value.year()) {
        return Err(TursoRepositoryError::InvalidData {
            field: "timestamp",
            value: value.to_string(),
        });
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        value.nanosecond()
    ))
}

fn parse_timestamp(
    field: &'static str,
    value: String,
) -> Result<OffsetDateTime, TursoRepositoryError> {
    OffsetDateTime::parse(&value, &Rfc3339)
        .map_err(|_| TursoRepositoryError::InvalidData { field, value })
}

fn parse_id<T>(field: &'static str, value: String) -> Result<T, TursoRepositoryError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| TursoRepositoryError::InvalidData { field, value })
}

#[allow(clippy::needless_pass_by_value)]
fn database_error(error: turso::Error) -> RepositoryError {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed") {
        RepositoryError::Conflict(message)
    } else {
        RepositoryError::Unavailable(message)
    }
}

fn repository_error(error: TursoRepositoryError) -> RepositoryError {
    match error {
        TursoRepositoryError::Database(error) => database_error(error),
        TursoRepositoryError::InvalidData { field, value } => {
            RepositoryError::CorruptData(format!("{field}: {value}"))
        }
        error => RepositoryError::Other(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use pah_application::{LibraryRepository, SearchCriteria, SearchKind};
    use pah_domain::{
        Asset, AssetId, AssetLink, AssetLinkId, AssetRelationType, Evaluation, EvaluationId,
        GenerationRun, GenerationRunId, GenerationSource, InputSnapshot, JsonObject, MediaType,
        Project, ProjectId, Prompt, PromptId, PromptVariant, PromptVariantId,
    };
    use time::{Duration, OffsetDateTime};

    use super::TursoLibraryRepository;

    #[tokio::test]
    async fn opens_schema_and_searches_an_asset() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/test");
        std::fs::create_dir_all(&root).expect("workspace test directory");
        let directory = tempfile::Builder::new()
            .prefix("turso-")
            .tempdir_in(root)
            .expect("temporary database directory");
        let repository = TursoLibraryRepository::open_local(directory.path().join("library.db"))
            .await
            .expect("open Turso repository");
        let now = OffsetDateTime::now_utc();
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("local tenant");
        let asset = Asset::new_external(
            AssetId::new(),
            tenant_id,
            MediaType::Image,
            "https://example.test/result.png",
            Some("result.png".into()),
            Some("image/png".into()),
            Some(1024),
            Some(1024),
            None,
            now,
        )
        .expect("asset");
        repository
            .create_asset(tenant_id, asset.clone(), Vec::new())
            .await
            .expect("create asset");
        repository
            .replace_asset_tags(
                tenant_id,
                asset.id,
                vec!["portrait".into(), "warm-light".into()],
                now + Duration::seconds(1),
            )
            .await
            .expect("save asset tags");
        let page = repository
            .search_library(
                tenant_id,
                SearchCriteria {
                    query: Some("result".into()),
                    kinds: vec![SearchKind::Asset],
                    project_id: None,
                    providers: Vec::new(),
                    models: Vec::new(),
                    media_types: Vec::new(),
                    prompt_statuses: Vec::new(),
                    minimum_score: None,
                    featured_only: false,
                    tags: vec!["portrait".into(), "warm-light".into()],
                    include_archived: false,
                    limit: 20,
                    cursor: None,
                },
            )
            .await
            .expect("search");
        assert_eq!(page.items.len(), 1);
        let mut projected_tags = page.items[0].tags.clone();
        projected_tags.sort();
        assert_eq!(projected_tags, vec!["portrait", "warm-light"]);
        let detail = repository
            .get_asset_detail(tenant_id, asset.id)
            .await
            .expect("asset detail")
            .expect("asset exists");
        assert_eq!(detail.tags, vec!["portrait", "warm-light"]);
    }

    #[tokio::test]
    async fn reconstructs_overview_and_asset_lineage() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/test");
        std::fs::create_dir_all(&root).expect("workspace test directory");
        let directory = tempfile::Builder::new()
            .prefix("turso-lineage-")
            .tempdir_in(root)
            .expect("temporary database directory");
        let repository = TursoLibraryRepository::open_local(directory.path().join("library.db"))
            .await
            .expect("open Turso repository");
        let now = OffsetDateTime::UNIX_EPOCH;
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("local tenant");
        let project =
            Project::new(ProjectId::new(), tenant_id, "Campaign", None, now).expect("project");
        repository
            .create_project(tenant_id, project.clone())
            .await
            .expect("create project");
        let prompt = Prompt::new(
            PromptId::new(),
            tenant_id,
            project.id,
            "Product hero",
            "Reusable commercial composition",
            "A product in warm window light",
            None,
            "local-user",
            now + Duration::seconds(1),
        )
        .expect("prompt");
        let variant = PromptVariant::new(
            PromptVariantId::new(),
            tenant_id,
            prompt.id,
            "openai",
            "gpt-image-2",
            None,
            "Render a product in warm window light",
            BTreeMap::new(),
            None,
            None,
            None,
            now + Duration::seconds(1),
        )
        .expect("variant");
        repository
            .create_prompt_bundle(tenant_id, prompt.clone(), Some(variant.clone()))
            .await
            .expect("create prompt bundle");
        let asset = Asset::new_external(
            AssetId::new(),
            tenant_id,
            MediaType::Image,
            "https://example.test/output.png",
            Some("output.png".into()),
            Some("image/png".into()),
            Some(1024),
            Some(1024),
            None,
            now + Duration::seconds(2),
        )
        .expect("asset");
        repository
            .create_asset(tenant_id, asset.clone(), Vec::new())
            .await
            .expect("create asset");
        let mut run = GenerationRun::new(
            GenerationRunId::new(),
            tenant_id,
            variant.id,
            GenerationSource::Manual,
            InputSnapshot::new(&variant.prompt_text, Vec::new(), JsonObject::new())
                .expect("input snapshot"),
            JsonObject::new(),
            None,
            None,
            "local-user",
            now + Duration::seconds(3),
        )
        .expect("generation");
        run.succeed(now + Duration::seconds(4))
            .expect("complete generation");
        let link = AssetLink::new(
            AssetLinkId::new(),
            tenant_id,
            asset.id,
            Some(run.id),
            Some(prompt.id),
            None,
            AssetRelationType::Output,
            0,
            None,
            now + Duration::seconds(4),
        )
        .expect("link");
        repository
            .record_generation(tenant_id, run.clone(), vec![link])
            .await
            .expect("record generation");
        let evaluation = Evaluation::new(
            EvaluationId::new(),
            tenant_id,
            asset.id,
            Some(run.id),
            5,
            true,
            true,
            BTreeMap::new(),
            None,
            None,
            Some("Keep the restrained lighting".into()),
            "local-user",
            now + Duration::seconds(5),
        )
        .expect("evaluation");
        repository
            .create_evaluation(tenant_id, evaluation.clone())
            .await
            .expect("create evaluation");

        let overview = repository
            .get_library_overview(tenant_id)
            .await
            .expect("overview");
        assert_eq!(overview.projects, vec![project]);
        assert_eq!(overview.prompts, vec![prompt.clone()]);
        assert_eq!(overview.variants, vec![variant.clone()]);
        assert_eq!(overview.variant_result_counts.get(&variant.id), Some(&1));

        let detail = repository
            .get_asset_detail(tenant_id, asset.id)
            .await
            .expect("asset detail")
            .expect("asset exists");
        assert_eq!(detail.asset, asset);
        assert_eq!(detail.prompts, vec![prompt]);
        assert_eq!(detail.variants, vec![variant]);
        assert_eq!(detail.generation_runs, vec![run]);
        assert_eq!(detail.evaluations, vec![evaluation]);
    }
}
