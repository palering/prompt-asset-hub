mod config;

use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use pah_application::{
    AppService, AssetImportSource, CreateEvaluationRequest, CreateProjectRequest,
    CreatePromptRequest, CreatePromptVariantRequest, GenerationRunDto, ImportAssetRequest,
    LibraryRepository, RecordGenerationRequest, RequestContext, SearchKind, SearchLibraryRequest,
    SearchRecordId, UpdateAssetContextRequest, UpdateAssetTagsRequest,
};
use pah_db_sqlite::SqliteLibraryRepository;
use pah_db_turso::TursoLibraryRepository;
use pah_domain::{AssetId, AssetLocation, MediaType};
use pah_storage_local::LocalBlobStore;
use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone)]
struct LibraryState {
    service: AppService,
    context: RequestContext,
    blob_store: Arc<LocalBlobStore>,
    data_dir: PathBuf,
    config_path: PathBuf,
    config_base_dir: PathBuf,
    config_lock: Arc<Mutex<()>>,
    database: &'static str,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    storage: &'static str,
    database: &'static str,
}

#[derive(Debug, Serialize)]
struct AppSettingsResponse {
    config_version: u32,
    config_path: String,
    ui: UiSettingsView,
    storage: StorageSettingsView,
}

#[derive(Debug, Serialize)]
struct UiSettingsView {
    default_asset_view: String,
    sidebar_collapsed: bool,
    sidebar_width: u16,
}

#[derive(Debug, Serialize)]
struct StorageSettingsView {
    backend: &'static str,
    source: &'static str,
    active_data_root: String,
    configured_data_root: String,
    database_path: String,
    media_path: String,
    exports_path: String,
    temp_path: String,
    restart_required: bool,
}

#[derive(Debug, serde::Deserialize)]
struct UpdateAppSettingsRequest {
    ui: UpdateUiSettingsRequest,
}

#[derive(Debug, serde::Deserialize)]
struct UpdateUiSettingsRequest {
    default_asset_view: String,
    sidebar_collapsed: bool,
    sidebar_width: u16,
}

#[derive(Debug, Serialize)]
struct LibraryCounts {
    assets: usize,
    prompts: usize,
    projects: usize,
    uncategorized: usize,
}

#[derive(Debug, Serialize)]
struct BootstrapResponse {
    library_name: &'static str,
    initialized: bool,
    storage_mode: &'static str,
    tenant_id: String,
    data_dir: String,
    counts: LibraryCounts,
    models: Vec<String>,
    projects: Vec<ProjectSummary>,
    prompts: Vec<PromptSummary>,
}

#[derive(Debug, Serialize)]
struct ProjectSummary {
    id: String,
    name: String,
    description: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct PromptSummary {
    id: String,
    project_id: String,
    title: String,
    intent: String,
    master_prompt: String,
    status: &'static str,
    tags: Vec<String>,
    updated_at: String,
    variants: Vec<PromptVariantSummary>,
}

#[derive(Debug, Serialize)]
struct PromptVariantSummary {
    id: String,
    model: String,
    platform: String,
    prompt: String,
    status: &'static str,
    rating: Option<f32>,
    result_count: u64,
    parameter_summary: Option<String>,
    adaptation_note: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssetSummary {
    id: String,
    title: String,
    media_type: &'static str,
    preview_url: Option<String>,
    model: Option<String>,
    prompt_title: Option<String>,
    project: Option<String>,
    relationship: Option<&'static str>,
    rating: Option<u8>,
    tags: Vec<String>,
    created_at: String,
    featured: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportAssetBytesRequest {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct AssetDetailView {
    id: String,
    original_url: String,
    original_filename: Option<String>,
    mime_type: Option<String>,
    byte_size: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    created_at: String,
    relation_type: Option<&'static str>,
    prompt: Option<AssetPromptView>,
    variant: Option<AssetVariantView>,
    generation: Option<AssetGenerationView>,
    evaluation: Option<AssetEvaluationView>,
    related_asset_count: usize,
    tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AssetPromptView {
    id: String,
    project_id: String,
    title: String,
    creative_intent: String,
    base_prompt: String,
    negative_constraints: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssetVariantView {
    id: String,
    provider: String,
    model: String,
    prompt_text: String,
    parameters: BTreeMap<String, serde_json::Value>,
    adaptation_notes: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssetGenerationView {
    id: String,
    source: &'static str,
    status: &'static str,
    prompt_text_snapshot: String,
    parameters_snapshot: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AssetEvaluationView {
    overall_score: u8,
    is_featured: bool,
    is_reusable: bool,
    strengths: Option<String>,
    issues: Option<String>,
    lesson: Option<String>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn health(state: tauri::State<'_, LibraryState>) -> HealthResponse {
    health_for_database(state.database)
}

const fn health_for_database(database: &'static str) -> HealthResponse {
    HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        storage: "local_filesystem",
        database,
    }
}

fn normalized_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn app_settings_response(
    state: &LibraryState,
    app_config: &config::AppConfig,
) -> AppSettingsResponse {
    let configured_data_root = normalized_path(&config::resolve_data_root(
        &state.config_base_dir,
        &app_config.storage.data_root,
    ));
    let active_data_root = normalized_path(&state.data_dir);
    let database_filename = if state.database == "turso-local" {
        "library.turso"
    } else {
        "library.sqlite3"
    };
    let source = if std::env::var_os("PAH_DATA_DIR").is_some() {
        "environment"
    } else {
        "config"
    };

    AppSettingsResponse {
        config_version: config::CONFIG_VERSION,
        config_path: config::path_text(&state.config_path),
        ui: UiSettingsView {
            default_asset_view: app_config.ui.default_asset_view.clone(),
            sidebar_collapsed: app_config.ui.sidebar_collapsed,
            sidebar_width: app_config.ui.sidebar_width,
        },
        storage: StorageSettingsView {
            backend: state.database,
            source,
            active_data_root: config::path_text(&active_data_root),
            configured_data_root: config::path_text(&configured_data_root),
            database_path: config::path_text(&active_data_root.join(database_filename)),
            media_path: config::path_text(&active_data_root.join("media")),
            exports_path: config::path_text(&active_data_root.join("exports")),
            temp_path: config::path_text(&active_data_root.join("tmp")),
            restart_required: active_data_root != configured_data_root,
        },
    }
}

fn lock_config(state: &LibraryState) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    state
        .config_lock
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_app_settings(state: tauri::State<'_, LibraryState>) -> Result<AppSettingsResponse, String> {
    let _guard = lock_config(&state)?;
    let app_config = config::load(&state.config_path)?;
    Ok(app_settings_response(&state, &app_config))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn update_app_settings(
    request: UpdateAppSettingsRequest,
    state: tauri::State<'_, LibraryState>,
) -> Result<AppSettingsResponse, String> {
    let _guard = lock_config(&state)?;
    let mut app_config = config::load(&state.config_path)?;
    app_config.ui.default_asset_view = request.ui.default_asset_view;
    app_config.ui.sidebar_collapsed = request.ui.sidebar_collapsed;
    app_config.ui.sidebar_width = request.ui.sidebar_width;
    app_config.normalize();
    config::save(&state.config_path, &app_config)?;
    Ok(app_settings_response(&state, &app_config))
}

#[tauri::command]
async fn choose_data_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, LibraryState>,
) -> Result<Option<AppSettingsResponse>, String> {
    let selected = app
        .dialog()
        .file()
        .set_directory(&state.data_dir)
        .blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let selected = selected
        .into_path()
        .map_err(|error| error.to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !selected.is_dir() {
        return Err("selected data root is not a directory".to_owned());
    }

    let _guard = lock_config(&state)?;
    let mut app_config = config::load(&state.config_path)?;
    app_config.storage.data_root = config::path_text(&selected);
    config::save(&state.config_path, &app_config)?;
    Ok(Some(app_settings_response(&state, &app_config)))
}

#[tauri::command]
async fn bootstrap_library(
    state: tauri::State<'_, LibraryState>,
) -> Result<BootstrapResponse, String> {
    let assets = search(&state, None, vec![SearchKind::Asset]).await?;
    let overview = state
        .service
        .get_library_overview(&state.context)
        .await
        .map_err(|error| error.to_string())?;

    let mut models: Vec<String> = overview
        .variants
        .iter()
        .map(|variant| variant.model.clone())
        .collect();
    models.sort();
    models.dedup();

    let projects = overview
        .projects
        .iter()
        .map(|project| {
            Ok(ProjectSummary {
                id: project.id.to_string(),
                name: project.name.clone(),
                description: project.description.clone(),
                updated_at: format_time(project.updated_at)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let prompts = overview
        .prompts
        .iter()
        .map(|prompt| {
            let variants = overview
                .variants
                .iter()
                .filter(|variant| variant.prompt_id == prompt.id)
                .map(|variant| {
                    Ok(PromptVariantSummary {
                        id: variant.id.to_string(),
                        model: variant.model.clone(),
                        platform: variant.provider.clone(),
                        prompt: variant.prompt_text.clone(),
                        status: prompt_status_name(variant.status),
                        rating: None,
                        result_count: overview
                            .variant_result_counts
                            .get(&variant.id)
                            .copied()
                            .unwrap_or_default(),
                        parameter_summary: (!variant.parameters.is_empty())
                            .then(|| serde_json::to_string(&variant.parameters))
                            .transpose()
                            .map_err(|error| error.to_string())?,
                        adaptation_note: variant.adaptation_notes.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(PromptSummary {
                id: prompt.id.to_string(),
                project_id: prompt.project_id.to_string(),
                title: prompt.title.clone(),
                intent: prompt.creative_intent.clone(),
                master_prompt: prompt.base_prompt.clone(),
                status: prompt_status_name(prompt.status),
                tags: Vec::new(),
                updated_at: format_time(prompt.updated_at)?,
                variants,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let uncategorized = assets
        .iter()
        .filter(|asset| asset.prompt_title.is_none())
        .count();

    Ok(BootstrapResponse {
        library_name: "My Creative Library",
        initialized: true,
        storage_mode: "local",
        tenant_id: state.context.tenant_id.to_string(),
        data_dir: state.data_dir.display().to_string(),
        counts: LibraryCounts {
            assets: assets.len(),
            prompts: prompts.len(),
            projects: projects.len(),
            uncategorized,
        },
        models,
        projects,
        prompts,
    })
}

#[tauri::command]
async fn create_project(
    state: tauri::State<'_, LibraryState>,
    request: CreateProjectRequest,
) -> Result<pah_application::ProjectDto, String> {
    state
        .service
        .create_project(&state.context, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_prompt(
    state: tauri::State<'_, LibraryState>,
    request: CreatePromptRequest,
) -> Result<pah_application::CreatedPromptDto, String> {
    state
        .service
        .create_prompt(&state.context, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_prompt_variant(
    state: tauri::State<'_, LibraryState>,
    request: CreatePromptVariantRequest,
) -> Result<pah_application::PromptVariantDto, String> {
    state
        .service
        .create_prompt_variant(&state.context, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn record_generation(
    state: tauri::State<'_, LibraryState>,
    request: RecordGenerationRequest,
) -> Result<GenerationRunDto, String> {
    state
        .service
        .record_generation(&state.context, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_evaluation(
    state: tauri::State<'_, LibraryState>,
    request: CreateEvaluationRequest,
) -> Result<pah_application::EvaluationDto, String> {
    state
        .service
        .create_evaluation(&state.context, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn update_asset_context(
    state: tauri::State<'_, LibraryState>,
    request: UpdateAssetContextRequest,
) -> Result<AssetDetailView, String> {
    let asset_id = request.asset_id;
    state
        .service
        .update_asset_context(&state.context, request)
        .await
        .map_err(|error| error.to_string())?;
    get_asset_detail(state, asset_id.to_string()).await
}

#[tauri::command]
async fn update_asset_tags(
    state: tauri::State<'_, LibraryState>,
    request: UpdateAssetTagsRequest,
) -> Result<AssetDetailView, String> {
    let asset_id = request.asset_id;
    state
        .service
        .update_asset_tags(&state.context, request)
        .await
        .map_err(|error| error.to_string())?;
    get_asset_detail(state, asset_id.to_string()).await
}

#[tauri::command]
async fn get_asset_detail(
    state: tauri::State<'_, LibraryState>,
    asset_id: String,
) -> Result<AssetDetailView, String> {
    let asset_id = AssetId::from_str(&asset_id).map_err(|_| "素材 ID 格式无效".to_owned())?;
    let detail = state
        .service
        .get_asset_detail(&state.context, asset_id)
        .await
        .map_err(|error| error.to_string())?;
    let generation = detail.generation_runs.first();
    let variant = generation
        .and_then(|run| {
            detail
                .variants
                .iter()
                .find(|variant| variant.id == run.variant_id)
        })
        .or_else(|| detail.variants.first());
    let prompt = variant
        .and_then(|variant| {
            detail
                .prompts
                .iter()
                .find(|prompt| prompt.id == variant.prompt_id)
        })
        .or_else(|| detail.prompts.first());
    let evaluation = detail.evaluations.first();
    let original_url = match &detail.asset.location {
        AssetLocation::Stored { storage_key } => media_url(storage_key),
        AssetLocation::External { url } => url.clone(),
    };
    Ok(AssetDetailView {
        id: detail.asset.id.to_string(),
        original_url,
        original_filename: detail.asset.original_filename,
        mime_type: detail.asset.mime_type,
        byte_size: detail.asset.byte_size,
        width: detail.asset.width,
        height: detail.asset.height,
        duration_ms: detail.asset.duration_ms,
        created_at: format_time(detail.asset.created_at)?,
        relation_type: detail
            .links
            .first()
            .map(|link| asset_relation_name(link.relation_type)),
        prompt: prompt.map(|prompt| AssetPromptView {
            id: prompt.id.to_string(),
            project_id: prompt.project_id.to_string(),
            title: prompt.title.clone(),
            creative_intent: prompt.creative_intent.clone(),
            base_prompt: prompt.base_prompt.clone(),
            negative_constraints: prompt.negative_constraints.clone(),
        }),
        variant: variant.map(|variant| AssetVariantView {
            id: variant.id.to_string(),
            provider: variant.provider.clone(),
            model: variant.model.clone(),
            prompt_text: variant.prompt_text.clone(),
            parameters: variant.parameters.clone(),
            adaptation_notes: variant.adaptation_notes.clone(),
        }),
        generation: generation.map(|run| AssetGenerationView {
            id: run.id.to_string(),
            source: generation_source_name(run.source),
            status: generation_status_name(run.status),
            prompt_text_snapshot: run.prompt_text_snapshot.clone(),
            parameters_snapshot: run.parameters_snapshot.clone(),
        }),
        evaluation: evaluation.map(|evaluation| AssetEvaluationView {
            overall_score: evaluation.overall_score,
            is_featured: evaluation.is_featured,
            is_reusable: evaluation.is_reusable,
            strengths: evaluation.strengths.clone(),
            issues: evaluation.issues.clone(),
            lesson: evaluation.lesson.clone(),
        }),
        related_asset_count: detail.related_assets.len(),
        tags: detail.tags,
    })
}

#[tauri::command]
async fn list_assets(state: tauri::State<'_, LibraryState>) -> Result<Vec<AssetSummary>, String> {
    search(&state, None, vec![SearchKind::Asset]).await
}

#[tauri::command]
async fn search_library(
    state: tauri::State<'_, LibraryState>,
    query: String,
) -> Result<Vec<AssetSummary>, String> {
    let query = (!query.trim().is_empty()).then_some(query);
    search(&state, query, vec![SearchKind::Asset]).await
}

async fn search(
    state: &LibraryState,
    query: Option<String>,
    kinds: Vec<SearchKind>,
) -> Result<Vec<AssetSummary>, String> {
    let results = state
        .service
        .search_library(
            &state.context,
            SearchLibraryRequest {
                query,
                kinds,
                limit: Some(500),
                ..SearchLibraryRequest::default()
            },
        )
        .await
        .map_err(|error| error.to_string())?;

    let mut assets = Vec::new();
    for item in results.items {
        let SearchRecordId::Asset(id) = item.id else {
            continue;
        };
        let Some(media_type) = item.media_type.map(media_type_name) else {
            continue;
        };
        let relationship = item.summary.as_ref().map(|_| "output");
        assets.push(AssetSummary {
            id: id.to_string(),
            title: item.title,
            media_type,
            preview_url: item.thumbnail_key.as_deref().map(media_url),
            model: item.model,
            prompt_title: item.summary,
            project: item.project_name,
            relationship,
            rating: item.overall_score,
            tags: item.tags,
            created_at: item
                .created_at
                .format(&Rfc3339)
                .map_err(|error| error.to_string())?,
            featured: item.is_featured,
        });
    }
    Ok(assets)
}

#[tauri::command]
async fn import_asset_bytes(
    state: tauri::State<'_, LibraryState>,
    request: ImportAssetBytesRequest,
) -> Result<AssetSummary, String> {
    let media_type = if request.mime_type.starts_with("image/") {
        MediaType::Image
    } else if request.mime_type.starts_with("video/") {
        MediaType::Video
    } else {
        return Err("当前导入只接受图片或视频".into());
    };
    let staged = state
        .blob_store
        .stage_bytes(
            &request.bytes,
            Some(request.filename.clone()),
            request.mime_type,
        )
        .await
        .map_err(|error| error.to_string())?;
    let asset = state
        .service
        .import_asset(
            &state.context,
            ImportAssetRequest {
                media_type,
                source: AssetImportSource::Staged { blob: staged },
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(AssetSummary {
        id: asset.id.to_string(),
        title: asset
            .original_filename
            .unwrap_or_else(|| "未命名素材".to_owned()),
        media_type: media_type_name(asset.media_type),
        preview_url: asset.thumbnail_key.as_deref().map(media_url),
        model: None,
        prompt_title: None,
        project: None,
        relationship: None,
        rating: None,
        tags: Vec::new(),
        created_at: asset
            .created_at
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?,
        featured: false,
    })
}

fn media_url(storage_key: &str) -> String {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        format!("http://pah-media.localhost/{storage_key}")
    } else {
        format!("pah-media://localhost/{storage_key}")
    }
}

const fn media_type_name(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Image => "image",
        MediaType::Video => "video",
        MediaType::Reference | MediaType::Frame | MediaType::Document => "reference",
    }
}

fn format_time(value: OffsetDateTime) -> Result<String, String> {
    value.format(&Rfc3339).map_err(|error| error.to_string())
}

const fn prompt_status_name(status: pah_domain::PromptStatus) -> &'static str {
    match status {
        pah_domain::PromptStatus::Draft => "draft",
        pah_domain::PromptStatus::Verified => "verified",
        pah_domain::PromptStatus::Archived => "archived",
    }
}

const fn asset_relation_name(relation: pah_domain::AssetRelationType) -> &'static str {
    match relation {
        pah_domain::AssetRelationType::Output => "output",
        pah_domain::AssetRelationType::ReferenceInput => "reference",
        pah_domain::AssetRelationType::Inspiration => "inspiration",
        pah_domain::AssetRelationType::FirstFrame
        | pah_domain::AssetRelationType::LastFrame
        | pah_domain::AssetRelationType::DerivedFrom => "derived",
    }
}

const fn generation_source_name(source: pah_domain::GenerationSource) -> &'static str {
    match source {
        pah_domain::GenerationSource::Manual => "manual",
        pah_domain::GenerationSource::Extension => "extension",
        pah_domain::GenerationSource::Api => "api",
        pah_domain::GenerationSource::Agent => "agent",
    }
}

const fn generation_status_name(status: pah_domain::GenerationStatus) -> &'static str {
    match status {
        pah_domain::GenerationStatus::Pending => "pending",
        pah_domain::GenerationStatus::Running => "running",
        pah_domain::GenerationStatus::Succeeded => "succeeded",
        pah_domain::GenerationStatus::Failed => "failed",
        pah_domain::GenerationStatus::Cancelled => "cancelled",
    }
}

async fn initialize_state(
    data_dir: PathBuf,
    backend: &str,
    config_path: PathBuf,
    config_base_dir: PathBuf,
) -> Result<LibraryState, String> {
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|error| error.to_string())?;
    let data_dir = data_dir.canonicalize().map_err(|error| error.to_string())?;
    tokio::fs::create_dir_all(data_dir.join("exports"))
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::create_dir_all(data_dir.join("tmp"))
        .await
        .map_err(|error| error.to_string())?;

    let (repository, tenant_id, database): (
        Arc<dyn LibraryRepository>,
        pah_domain::TenantId,
        &'static str,
    ) = match backend {
        "sqlite" => {
            let repository = SqliteLibraryRepository::open(data_dir.join("library.sqlite3"))
                .await
                .map_err(|error| error.to_string())?;
            let tenant_id = repository
                .ensure_local_tenant(OffsetDateTime::now_utc())
                .await
                .map_err(|error| error.to_string())?;
            (Arc::new(repository), tenant_id, "sqlite")
        }
        "turso" => {
            let repository = TursoLibraryRepository::open_local(data_dir.join("library.turso"))
                .await
                .map_err(|error| error.to_string())?;
            let tenant_id = repository
                .ensure_local_tenant(OffsetDateTime::now_utc())
                .await
                .map_err(|error| error.to_string())?;
            (Arc::new(repository), tenant_id, "turso-local")
        }
        value => {
            return Err(format!(
                "unsupported PAH_DATABASE_BACKEND '{value}'; use sqlite or turso"
            ));
        }
    };
    let blob_store = Arc::new(
        LocalBlobStore::open(data_dir.join("media"))
            .await
            .map_err(|error| error.to_string())?,
    );

    Ok(LibraryState {
        service: AppService::new(repository, blob_store.clone()),
        context: RequestContext::local(tenant_id),
        blob_store,
        data_dir,
        config_path,
        config_base_dir,
        config_lock: Arc::new(Mutex::new(())),
        database,
    })
}

fn media_response(state: &LibraryState, request_path: &str) -> tauri::http::Response<Vec<u8>> {
    let key = request_path.trim_start_matches('/');
    let path = Path::new(key);
    let expected_prefix = format!("tenants/{}/", state.context.tenant_id);
    let safe = key.starts_with(&expected_prefix)
        && !key.is_empty()
        && !path.is_absolute()
        && path.components().all(|component| {
            !matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        });
    if !safe {
        return tauri::http::Response::builder()
            .status(403)
            .body(Vec::new())
            .expect("valid forbidden response");
    }
    match std::fs::read(state.data_dir.join("media").join(path)) {
        Ok(bytes) => tauri::http::Response::builder()
            .header("Content-Type", content_type_for_key(key))
            .header("Cache-Control", "private, max-age=31536000, immutable")
            .body(bytes)
            .expect("valid media response"),
        Err(_) => tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .expect("valid not found response"),
    }
}

fn content_type_for_key(key: &str) -> &'static str {
    match Path::new(key).extension().and_then(|value| value.to_str()) {
        Some("webp") => "image/webp",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("avif") => "image/avif",
        Some("gif") => "image/gif",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the desktop application and initializes the local library.
///
/// # Panics
///
/// Panics when Tauri cannot create its runtime or the local database and
/// media directories cannot be initialized.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("pah-media", |context, request| {
            let state = context.app_handle().state::<LibraryState>();
            media_response(&state, request.uri().path())
        })
        .setup(|app| {
            let location = config::locate(app.handle()).map_err(std::io::Error::other)?;
            let app_config = config::load_or_create(&location).map_err(std::io::Error::other)?;
            let configured_data_root =
                config::resolve_data_root(&location.base_dir, &app_config.storage.data_root);
            let data_dir = std::env::var("PAH_DATA_DIR").map_or(configured_data_root, |value| {
                config::resolve_data_root(&location.base_dir, &value)
            });
            let backend = std::env::var("PAH_DATABASE_BACKEND")
                .unwrap_or(app_config.storage.database_backend)
                .to_ascii_lowercase();
            let state = tauri::async_runtime::block_on(initialize_state(
                data_dir,
                &backend,
                location.config_path,
                location.base_dir,
            ))
            .map_err(std::io::Error::other)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            get_app_settings,
            update_app_settings,
            choose_data_root,
            bootstrap_library,
            create_project,
            create_prompt,
            create_prompt_variant,
            list_assets,
            search_library,
            import_asset_bytes,
            record_generation,
            create_evaluation,
            update_asset_context,
            update_asset_tags,
            get_asset_detail
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Prompt Asset Hub");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{health_for_database, initialize_state, media_type_name};
    use pah_application::{SearchKind, SearchLibraryRequest};
    use pah_domain::MediaType;

    #[test]
    fn exposes_local_health_contract() {
        let health = health_for_database("sqlite");
        assert_eq!(health.status, "ok");
        assert_eq!(health.database, "sqlite");
        assert_eq!(media_type_name(MediaType::Image), "image");
    }

    #[tokio::test]
    async fn initializes_a_real_workspace_scoped_local_library() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.local/test");
        std::fs::create_dir_all(&root).expect("workspace test directory");
        let directory = tempfile::Builder::new()
            .prefix("desktop-")
            .tempdir_in(root)
            .expect("temporary app data directory");
        let state = initialize_state(
            directory.path().to_path_buf(),
            "sqlite",
            directory.path().join("config.toml"),
            directory.path().to_path_buf(),
        )
        .await
        .expect("initialize local state");

        assert!(directory.path().join("library.sqlite3").exists());
        assert!(directory.path().join("media").is_dir());

        let result = state
            .service
            .search_library(
                &state.context,
                SearchLibraryRequest {
                    kinds: vec![SearchKind::Asset],
                    ..SearchLibraryRequest::default()
                },
            )
            .await
            .expect("search empty library");
        assert!(result.items.is_empty());
    }
}
