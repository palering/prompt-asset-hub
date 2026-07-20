#![allow(clippy::needless_raw_string_hashes, clippy::too_many_lines)]

//! `SQLite` persistence adapter for the local Prompt Asset Hub library.
//!
//! The adapter owns connection pragmas and migrations. Callers work with
//! tenant-scoped application ports; raw SQL and the pool are intentionally not
//! exposed to the desktop `WebView` or external agents.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use pah_domain::{Tenant, TenantId};
use serde::{Serialize, de::DeserializeOwned};
use sqlx::{
    Row, SqlitePool,
    migrate::{MigrateError, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

mod repository;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations/sqlite");

const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_CONNECTIONS: u32 = 4;

#[derive(Debug, Error)]
pub enum SqliteRepositoryError {
    #[error("cannot prepare SQLite database directory {path}: {source}")]
    PrepareDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SQLite operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("SQLite migration failed: {0}")]
    Migration(#[from] MigrateError),
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

/// A cloneable SQLite-backed implementation of the application repository.
#[derive(Debug, Clone)]
pub struct SqliteLibraryRepository {
    pool: SqlitePool,
}

impl SqliteLibraryRepository {
    /// Opens (or creates) a local database and applies all embedded migrations.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent directory cannot be created, the
    /// database cannot be opened, or an embedded migration cannot be applied.
    pub async fn open(database_path: impl AsRef<Path>) -> Result<Self, SqliteRepositoryError> {
        let database_path = database_path.as_ref();
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| {
                SqliteRepositoryError::PrepareDirectory {
                    path: parent.to_path_buf(),
                    source,
                }
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(DEFAULT_BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .max_connections(DEFAULT_MAX_CONNECTIONS)
            .connect_with(options)
            .await?;

        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    /// Returns the existing hidden local tenant or creates it on first launch.
    ///
    /// # Errors
    ///
    /// Returns an error when the tenant row cannot be read, decoded, or
    /// inserted.
    pub async fn ensure_local_tenant(
        &self,
        now: OffsetDateTime,
    ) -> Result<TenantId, SqliteRepositoryError> {
        if let Some(row) = sqlx::query(
            "SELECT id FROM tenants WHERE slug = 'local' AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        {
            return parse_id("tenants.id", row.try_get::<String, _>("id")?);
        }

        let tenant = Tenant::local(TenantId::new(), now);
        self.upsert_tenant(&tenant).await?;
        Ok(tenant.id)
    }

    /// Inserts a tenant if it has not been initialized yet.
    ///
    /// # Errors
    ///
    /// Returns an error when enum/timestamp encoding fails or `SQLite` rejects
    /// the upsert.
    pub async fn upsert_tenant(&self, tenant: &Tenant) -> Result<(), SqliteRepositoryError> {
        sqlx::query(
            r#"
            INSERT INTO tenants (
                id, slug, name, status, created_at, updated_at, deleted_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                slug = excluded.slug,
                name = excluded.name,
                status = excluded.status,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at
            "#,
        )
        .bind(tenant.id.to_string())
        .bind(&tenant.slug)
        .bind(&tenant.name)
        .bind(encode_enum(&tenant.status)?)
        .bind(format_timestamp(tenant.timestamps.created_at)?)
        .bind(format_timestamp(tenant.timestamps.updated_at)?)
        .bind(format_optional_timestamp(tenant.timestamps.deleted_at)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn ensure_tenant(
    requested_tenant: TenantId,
    entity_tenant: TenantId,
) -> Result<(), SqliteRepositoryError> {
    if requested_tenant == entity_tenant {
        Ok(())
    } else {
        Err(SqliteRepositoryError::TenantMismatch {
            requested_tenant,
            entity_tenant,
        })
    }
}

fn encode_enum<T: Serialize>(value: &T) -> Result<String, SqliteRepositoryError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        value => Err(SqliteRepositoryError::InvalidData {
            field: "enum",
            value: value.to_string(),
        }),
    }
}

fn decode_enum<T: DeserializeOwned>(
    field: &'static str,
    value: String,
) -> Result<T, SqliteRepositoryError> {
    serde_json::from_value(serde_json::Value::String(value.clone()))
        .map_err(|_| SqliteRepositoryError::InvalidData { field, value })
}

fn format_timestamp(value: OffsetDateTime) -> Result<String, SqliteRepositoryError> {
    let value = value.to_offset(UtcOffset::UTC);
    if !(0..=9999).contains(&value.year()) {
        return Err(SqliteRepositoryError::InvalidData {
            field: "timestamp",
            value: value.to_string(),
        });
    }
    // A fixed nine-digit fractional component keeps TEXT ordering identical to
    // chronological ordering, which is required by cursor pagination.
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

fn format_optional_timestamp(
    value: Option<OffsetDateTime>,
) -> Result<Option<String>, SqliteRepositoryError> {
    value.map(format_timestamp).transpose()
}

fn parse_timestamp(
    field: &'static str,
    value: String,
) -> Result<OffsetDateTime, SqliteRepositoryError> {
    OffsetDateTime::parse(&value, &Rfc3339)
        .map_err(|_| SqliteRepositoryError::InvalidData { field, value })
}

fn parse_optional_timestamp(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<OffsetDateTime>, SqliteRepositoryError> {
    value.map(|value| parse_timestamp(field, value)).transpose()
}

fn parse_id<T>(field: &'static str, value: String) -> Result<T, SqliteRepositoryError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| SqliteRepositoryError::InvalidData { field, value })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use pah_application::{
        AssetContextReplacement, LibraryRepository, SearchCriteria, SearchKind, SearchRecordId,
    };
    use pah_domain::{
        Asset, AssetFile, AssetFileId, AssetFileRole, AssetId, AssetLink, AssetLinkId,
        AssetRelationType, Evaluation, EvaluationId, GenerationRun, GenerationRunId,
        GenerationSource, InputSnapshot, JsonObject, MediaType, Project, ProjectId, Prompt,
        PromptId, PromptStatus, PromptVariant, PromptVariantId,
    };
    use time::{Duration, OffsetDateTime};

    use super::SqliteLibraryRepository;

    async fn repository() -> (tempfile::TempDir, SqliteLibraryRepository) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/test");
        std::fs::create_dir_all(&root).expect("workspace test directory");
        let directory = tempfile::Builder::new()
            .prefix("sqlite-")
            .tempdir_in(root)
            .expect("temporary database directory");
        let repository = SqliteLibraryRepository::open(directory.path().join("library.sqlite3"))
            .await
            .expect("open SQLite repository");
        (directory, repository)
    }

    fn search_criteria(query: Option<&str>, limit: u32) -> SearchCriteria {
        SearchCriteria {
            query: query.map(str::to_owned),
            kinds: Vec::new(),
            project_id: None,
            providers: Vec::new(),
            models: Vec::new(),
            media_types: Vec::new(),
            prompt_statuses: Vec::new(),
            minimum_score: None,
            featured_only: false,
            tags: Vec::new(),
            include_archived: false,
            limit,
            cursor: None,
        }
    }

    #[tokio::test]
    async fn initializes_migrations_pragmas_and_one_stable_local_tenant() {
        let (_directory, repository) = repository().await;
        let now = OffsetDateTime::UNIX_EPOCH;
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("initialize tenant");
        let same_tenant_id = repository
            .ensure_local_tenant(now + Duration::seconds(1))
            .await
            .expect("reuse tenant");

        assert_eq!(tenant_id, same_tenant_id);
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(repository.pool())
            .await
            .expect("foreign key pragma");
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(repository.pool())
            .await
            .expect("journal mode pragma");
        let domain_table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN \
             ('tenants', 'projects', 'prompts', 'prompt_variants', 'generation_runs', \
              'assets', 'asset_links', 'evaluations', 'tags', 'audit_events')",
        )
        .fetch_one(repository.pool())
        .await
        .expect("migration tables");

        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode, "wal");
        assert_eq!(domain_table_count, 10);
    }

    #[tokio::test]
    async fn persists_original_and_thumbnail_in_one_asset_transaction() {
        let (_directory, repository) = repository().await;
        let now = OffsetDateTime::UNIX_EPOCH;
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("initialize tenant");
        let asset_id = AssetId::new();
        let hash = "a".repeat(64);
        let mut asset = Asset::new_stored(
            asset_id,
            tenant_id,
            MediaType::Image,
            format!("tenants/{tenant_id}/assets/{asset_id}/original.png"),
            Some("result.png".into()),
            "image/png",
            128,
            Some(1024),
            Some(768),
            None,
            hash.clone(),
            now,
        )
        .expect("stored asset");
        let thumbnail_key =
            format!("tenants/{tenant_id}/assets/{asset_id}/preview/thumbnail-480.webp");
        asset
            .set_thumbnail_key(thumbnail_key.clone())
            .expect("thumbnail key");
        let original = AssetFile::new(
            AssetFileId::new(),
            tenant_id,
            asset_id,
            AssetFileRole::Original,
            format!("tenants/{tenant_id}/assets/{asset_id}/original.png"),
            "image/png",
            128,
            Some(1024),
            Some(768),
            hash.clone(),
            None,
            now,
        )
        .expect("original file");
        let thumbnail = AssetFile::new(
            AssetFileId::new(),
            tenant_id,
            asset_id,
            AssetFileRole::Thumbnail,
            thumbnail_key,
            "image/webp",
            64,
            Some(480),
            Some(360),
            hash,
            Some("pah-image-webp-v1".into()),
            now,
        )
        .expect("thumbnail file");

        repository
            .create_asset(tenant_id, asset, vec![original, thumbnail])
            .await
            .expect("persist asset bundle");

        let file_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM asset_files WHERE tenant_id = ?")
                .bind(tenant_id.to_string())
                .fetch_one(repository.pool())
                .await
                .expect("count asset files");
        assert_eq!(file_count, 2);
    }

    #[tokio::test]
    async fn persists_searches_and_reconstructs_the_generation_lineage() {
        let (_directory, repository) = repository().await;
        let now = OffsetDateTime::UNIX_EPOCH;
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("initialize tenant");

        let project = Project::new(
            ProjectId::new(),
            tenant_id,
            "Launch Campaign",
            Some("Summer visuals".into()),
            now,
        )
        .expect("project");
        repository
            .create_project(tenant_id, project.clone())
            .await
            .expect("persist project");

        let prompt = Prompt::new(
            PromptId::new(),
            tenant_id,
            project.id,
            "Neon product hero",
            "A reusable product campaign composition",
            "A neon-lit product hero on a reflective table",
            Some("no watermark".into()),
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
            "Render a neon-lit product hero with cinematic reflections",
            JsonObject::new(),
            Some("Use declarative composition language".into()),
            None,
            None,
            now + Duration::seconds(1),
        )
        .expect("variant");
        repository
            .create_prompt_bundle(tenant_id, prompt.clone(), Some(variant.clone()))
            .await
            .expect("persist prompt bundle");

        let asset = Asset::new_external(
            AssetId::new(),
            tenant_id,
            MediaType::Image,
            "https://example.test/neon-output.png",
            Some("neon-output.png".into()),
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
            .expect("persist asset");

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
        .expect("generation run");
        run.succeed(now + Duration::seconds(4))
            .expect("complete generation");
        let link = AssetLink::new(
            AssetLinkId::new(),
            tenant_id,
            asset.id,
            Some(run.id),
            None,
            None,
            AssetRelationType::Output,
            0,
            None,
            now + Duration::seconds(4),
        )
        .expect("asset link");
        repository
            .record_generation(tenant_id, run.clone(), vec![link])
            .await
            .expect("persist generation lineage");

        let mut dimension_scores = BTreeMap::new();
        dimension_scores.insert("composition".into(), 5);
        let evaluation = Evaluation::new(
            EvaluationId::new(),
            tenant_id,
            asset.id,
            Some(run.id),
            5,
            true,
            true,
            dimension_scores,
            Some("Strong focal hierarchy".into()),
            None,
            Some("Reuse the reflective table wording".into()),
            "local-user",
            now + Duration::seconds(5),
        )
        .expect("evaluation");
        repository
            .create_evaluation(tenant_id, evaluation.clone())
            .await
            .expect("persist evaluation");

        let first_page = repository
            .search_library(tenant_id, search_criteria(Some("neon"), 1))
            .await
            .expect("search first page");
        assert_eq!(first_page.items.len(), 1);
        assert!(first_page.next_cursor.is_some());
        assert!(matches!(
            first_page.items[0].id,
            SearchRecordId::Asset(id) if id == asset.id
        ));

        let mut second_criteria = search_criteria(Some("neon"), 1);
        second_criteria.cursor = first_page.next_cursor;
        let second_page = repository
            .search_library(tenant_id, second_criteria)
            .await
            .expect("search second page");
        assert_eq!(second_page.items.len(), 1);
        assert!(matches!(
            second_page.items[0].id,
            SearchRecordId::Prompt(id) if id == prompt.id
        ));

        let mut filtered = search_criteria(None, 20);
        filtered.kinds = vec![SearchKind::Asset];
        filtered.providers = vec!["openai".into()];
        filtered.media_types = vec![MediaType::Image];
        filtered.minimum_score = Some(5);
        filtered.featured_only = true;
        let filtered_page = repository
            .search_library(tenant_id, filtered)
            .await
            .expect("structured asset search");
        assert_eq!(filtered_page.items.len(), 1);
        assert_eq!(filtered_page.items[0].overall_score, Some(5));

        let overview = repository
            .get_library_overview(tenant_id)
            .await
            .expect("library overview");
        assert_eq!(overview.projects, vec![project.clone()]);
        assert_eq!(overview.prompts, vec![prompt.clone()]);
        assert_eq!(overview.variants, vec![variant.clone()]);
        assert_eq!(overview.variant_result_counts.get(&variant.id), Some(&1));

        let prompt_detail = repository
            .get_prompt_detail(tenant_id, prompt.id)
            .await
            .expect("prompt detail")
            .expect("prompt exists");
        assert_eq!(prompt_detail.project, project);
        assert_eq!(prompt_detail.variants, vec![variant.clone()]);
        assert_eq!(prompt_detail.generation_runs, vec![run.clone()]);
        assert_eq!(prompt_detail.assets, vec![asset.clone()]);
        assert_eq!(prompt_detail.evaluations, vec![evaluation.clone()]);

        let asset_detail = repository
            .get_asset_detail(tenant_id, asset.id)
            .await
            .expect("asset detail")
            .expect("asset exists");
        assert_eq!(asset_detail.prompts, vec![prompt]);
        assert_eq!(asset_detail.variants, vec![variant]);
        assert_eq!(asset_detail.generation_runs, vec![run]);
        assert_eq!(asset_detail.evaluations, vec![evaluation]);

        let variant = asset_detail.variants[0].clone();
        let mut revised_run = GenerationRun::new(
            GenerationRunId::new(),
            tenant_id,
            variant.id,
            GenerationSource::Manual,
            InputSnapshot::new(
                "Revised prompt with a quieter neon reflection",
                Vec::new(),
                JsonObject::new(),
            )
            .expect("revised input snapshot"),
            JsonObject::new(),
            None,
            None,
            "local-user",
            now + Duration::seconds(6),
        )
        .expect("revised generation run");
        revised_run
            .succeed(now + Duration::seconds(7))
            .expect("complete revised generation");
        let revised_link = AssetLink::new(
            AssetLinkId::new(),
            tenant_id,
            asset.id,
            Some(revised_run.id),
            None,
            None,
            AssetRelationType::Output,
            0,
            None,
            now + Duration::seconds(7),
        )
        .expect("revised asset link");
        let revised_evaluation = Evaluation::new(
            EvaluationId::new(),
            tenant_id,
            asset.id,
            Some(revised_run.id),
            4,
            false,
            false,
            BTreeMap::new(),
            None,
            None,
            Some("The quieter reflection reads more naturally".into()),
            "local-user",
            now + Duration::seconds(8),
        )
        .expect("revised evaluation");
        repository
            .replace_asset_context(
                tenant_id,
                AssetContextReplacement {
                    asset_id: asset.id,
                    generation_run: Some(revised_run.clone()),
                    asset_link: Some(revised_link),
                    evaluation: Some(revised_evaluation.clone()),
                    replaced_at: now + Duration::seconds(8),
                },
            )
            .await
            .expect("replace active asset context");

        let revised_detail = repository
            .get_asset_detail(tenant_id, asset.id)
            .await
            .expect("revised asset detail")
            .expect("asset exists");
        assert_eq!(revised_detail.generation_runs, vec![revised_run]);
        assert_eq!(revised_detail.evaluations, vec![revised_evaluation]);

        let stored_run_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM generation_runs WHERE tenant_id = ?")
                .bind(tenant_id.to_string())
                .fetch_one(repository.pool())
                .await
                .expect("count immutable generation snapshots");
        let active_link_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM asset_links WHERE tenant_id = ? AND asset_id = ? \
             AND deleted_at IS NULL",
        )
        .bind(tenant_id.to_string())
        .bind(asset.id.to_string())
        .fetch_one(repository.pool())
        .await
        .expect("count active generation links");
        assert_eq!(stored_run_count, 2);
        assert_eq!(active_link_count, 1);

        repository
            .replace_asset_tags(
                tenant_id,
                asset.id,
                vec!["雨夜".into(), "人像".into()],
                now + Duration::seconds(9),
            )
            .await
            .expect("save asset tags");
        let tagged_detail = repository
            .get_asset_detail(tenant_id, asset.id)
            .await
            .expect("tagged asset detail")
            .expect("asset exists");
        assert_eq!(tagged_detail.tags, vec!["人像", "雨夜"]);

        let mut tagged_criteria = search_criteria(None, 20);
        tagged_criteria.kinds = vec![SearchKind::Asset];
        tagged_criteria.tags = vec!["雨夜".into(), "人像".into()];
        let tagged_page = repository
            .search_library(tenant_id, tagged_criteria)
            .await
            .expect("search by intersecting tags");
        assert_eq!(tagged_page.items.len(), 1);
        let mut projected_tags = tagged_page.items[0].tags.clone();
        projected_tags.sort();
        assert_eq!(projected_tags, vec!["人像", "雨夜"]);

        repository
            .replace_asset_tags(
                tenant_id,
                asset.id,
                vec!["商业静物".into()],
                now + Duration::seconds(10),
            )
            .await
            .expect("replace asset tags");
        let replaced_detail = repository
            .get_asset_detail(tenant_id, asset.id)
            .await
            .expect("retagged asset detail")
            .expect("asset exists");
        assert_eq!(replaced_detail.tags, vec!["商业静物"]);
    }

    #[tokio::test]
    async fn tenant_scope_is_enforced_before_writing() {
        let (_directory, repository) = repository().await;
        let now = OffsetDateTime::UNIX_EPOCH;
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("initialize tenant");
        let other_tenant_id = pah_domain::TenantId::new();
        let project = Project::new(ProjectId::new(), other_tenant_id, "Wrong tenant", None, now)
            .expect("project");

        let error = repository
            .create_project(tenant_id, project)
            .await
            .expect_err("cross-tenant write rejected");
        assert!(error.to_string().contains("does not match"));
    }

    #[tokio::test]
    async fn prompt_bundle_validation_is_atomic() {
        let (_directory, repository) = repository().await;
        let now = OffsetDateTime::UNIX_EPOCH;
        let tenant_id = repository
            .ensure_local_tenant(now)
            .await
            .expect("initialize tenant");
        let project =
            Project::new(ProjectId::new(), tenant_id, "Project", None, now).expect("project");
        repository
            .create_project(tenant_id, project.clone())
            .await
            .expect("persist project");
        let prompt = Prompt::new(
            PromptId::new(),
            tenant_id,
            project.id,
            "Prompt",
            "Intent",
            "Base prompt",
            None,
            "local-user",
            now,
        )
        .expect("prompt");
        let unrelated_variant = PromptVariant::new(
            PromptVariantId::new(),
            tenant_id,
            PromptId::new(),
            "openai",
            "gpt-image-2",
            None,
            "Executable prompt",
            JsonObject::new(),
            None,
            None,
            None,
            now,
        )
        .expect("variant");

        repository
            .create_prompt_bundle(tenant_id, prompt.clone(), Some(unrelated_variant))
            .await
            .expect_err("mismatched variant rejected");
        assert!(
            !repository
                .prompt_exists(tenant_id, prompt.id)
                .await
                .expect("prompt existence")
        );

        let mut archived_criteria = search_criteria(None, 10);
        archived_criteria.prompt_statuses = vec![PromptStatus::Archived];
        assert!(
            repository
                .search_library(tenant_id, archived_criteria)
                .await
                .expect("empty archived search")
                .items
                .is_empty()
        );
    }
}
