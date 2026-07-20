export type ClientMode = "mock" | "tauri";
export type MediaType = "image" | "video" | "reference";
export type AssetRelationship = "output" | "reference" | "inspiration" | "derived";
export type AssetSort = "newest" | "oldest" | "rating_high" | "rating_low" | "title";
export type PromptStatus = "draft" | "verified" | "archived";

export interface HealthStatus {
  status: "ok" | "degraded";
  version?: string;
  storage?: string;
  database?: string;
}

export interface AppSettings {
  config_version: number;
  config_path: string;
  ui: {
    default_asset_view: "card" | "list";
    sidebar_collapsed: boolean;
    sidebar_width: number;
  };
  storage: {
    backend: string;
    source: "config" | "environment" | "demo";
    active_data_root: string;
    configured_data_root: string;
    database_path: string;
    media_path: string;
    exports_path: string;
    temp_path: string;
    restart_required: boolean;
  };
}

export interface UpdateAppSettingsInput {
  ui: AppSettings["ui"];
}

export interface AssetSummary {
  id: string;
  title: string;
  media_type: MediaType;
  preview_url?: string | null;
  model?: string | null;
  prompt_title?: string | null;
  project?: string | null;
  relationship?: AssetRelationship;
  rating?: number | null;
  tags: string[];
  created_at: string;
  aspect_ratio?: string | null;
  duration_seconds?: number | null;
  featured?: boolean;
  palette?: string;
}

export interface PromptVariant {
  id: string;
  model: string;
  platform: string;
  prompt: string;
  status: PromptStatus;
  rating?: number | null;
  result_count: number;
  parameter_summary?: string;
  adaptation_note?: string;
}

export interface PromptSummary {
  id: string;
  project_id: string;
  title: string;
  intent: string;
  master_prompt: string;
  status: PromptStatus;
  tags: string[];
  updated_at: string;
  variants: PromptVariant[];
}

export interface ProjectSummary {
  id: string;
  name: string;
  description?: string | null;
  updated_at: string;
}

export interface LibraryCounts {
  assets: number;
  prompts: number;
  projects: number;
  uncategorized: number;
}

export interface LibraryBootstrap {
  library_name: string;
  initialized: boolean;
  storage_mode: "local" | "remote" | "demo";
  counts: LibraryCounts;
  models: string[];
  projects: ProjectSummary[];
  prompts: PromptSummary[];
}

export interface PromptVariantInput {
  provider: string;
  model: string;
  model_version?: string;
  prompt_text: string;
  parameters?: Record<string, unknown>;
  adaptation_notes?: string;
  success_tips?: string;
  failure_patterns?: string;
}

export interface CreatePromptInput {
  project_id: string;
  title: string;
  creative_intent: string;
  base_prompt: string;
  negative_constraints?: string;
  initial_variant?: PromptVariantInput;
}

export interface ImportAssetOptions {
  variant_id?: string;
  prompt_text?: string;
  parameters?: Record<string, unknown>;
  rating?: number;
  featured?: boolean;
  lesson?: string;
}

export interface AssetDetail {
  id: string;
  original_url?: string | null;
  original_filename?: string | null;
  mime_type?: string | null;
  byte_size?: number | null;
  width?: number | null;
  height?: number | null;
  duration_ms?: number | null;
  created_at: string;
  relation_type?: AssetRelationship | null;
  prompt?: {
    id: string;
    project_id: string;
    title: string;
    creative_intent: string;
    base_prompt: string;
    negative_constraints?: string | null;
  } | null;
  variant?: {
    id: string;
    provider: string;
    model: string;
    prompt_text: string;
    parameters: Record<string, unknown>;
    adaptation_notes?: string | null;
  } | null;
  generation?: {
    id: string;
    source: string;
    status: string;
    prompt_text_snapshot: string;
    parameters_snapshot: Record<string, unknown>;
  } | null;
  evaluation?: {
    overall_score: number;
    is_featured: boolean;
    is_reusable: boolean;
    strengths?: string | null;
    issues?: string | null;
    lesson?: string | null;
  } | null;
  related_asset_count: number;
  tags: string[];
}

export interface UpdateAssetContextInput {
  asset_id: string;
  variant_id?: string;
  prompt_text_snapshot?: string;
  parameters_snapshot?: Record<string, unknown>;
  overall_score?: number;
  is_featured: boolean;
  lesson?: string;
}

export interface UpdateAssetTagsInput {
  asset_id: string;
  tags: string[];
}

export interface AssetQuery {
  query?: string;
  media_type?: MediaType | "all";
  model?: string;
  min_rating?: number;
  featured_only?: boolean;
  tags?: string[];
  sort?: AssetSort;
  limit?: number;
}

export interface AssetPage {
  items: AssetSummary[];
  total: number;
}

export interface SearchResult extends AssetPage {
  prompts: PromptSummary[];
}

export interface LibraryClient {
  readonly mode: ClientMode;
  health(): Promise<HealthStatus>;
  getAppSettings(): Promise<AppSettings>;
  updateAppSettings(input: UpdateAppSettingsInput): Promise<AppSettings>;
  chooseDataRoot(): Promise<AppSettings | null>;
  bootstrap(): Promise<LibraryBootstrap>;
  listAssets(query?: AssetQuery): Promise<AssetPage>;
  searchLibrary(query: AssetQuery): Promise<SearchResult>;
  createProject(input: { name: string; description?: string }): Promise<void>;
  createPrompt(input: CreatePromptInput): Promise<void>;
  addPromptVariant(promptId: string, input: PromptVariantInput): Promise<void>;
  importAssets(files: File[], options?: ImportAssetOptions): Promise<AssetSummary[]>;
  getAssetDetail(assetId: string): Promise<AssetDetail>;
  updateAssetContext(input: UpdateAssetContextInput): Promise<AssetDetail>;
  updateAssetTags(input: UpdateAssetTagsInput): Promise<AssetDetail>;
}

export const emptyBootstrap = (storageMode: LibraryBootstrap["storage_mode"] = "local"): LibraryBootstrap => ({
  library_name: "My Creative Library",
  initialized: true,
  storage_mode: storageMode,
  counts: { assets: 0, prompts: 0, projects: 0, uncategorized: 0 },
  models: [],
  projects: [],
  prompts: [],
});

export function matchesAsset(asset: AssetSummary, query: AssetQuery): boolean {
  if (query.media_type && query.media_type !== "all" && asset.media_type !== query.media_type) return false;
  if (query.model && asset.model !== query.model) return false;
  if (query.min_rating && (asset.rating ?? 0) < query.min_rating) return false;
  if (query.featured_only && !asset.featured) return false;
  if (query.tags?.length) {
    const assetTags = new Set(asset.tags.map((tag) => tag.toLocaleLowerCase()));
    if (!query.tags.every((tag) => assetTags.has(tag.toLocaleLowerCase()))) return false;
  }

  const term = query.query?.trim().toLocaleLowerCase();
  if (!term) return true;

  const searchable = [
    asset.title,
    asset.model,
    asset.prompt_title,
    asset.project,
    ...asset.tags,
  ]
    .filter(Boolean)
    .join(" ")
    .toLocaleLowerCase();

  return searchable.includes(term);
}

export function sortAssets(assets: AssetSummary[], sort: AssetSort = "newest"): AssetSummary[] {
  const collator = new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" });
  const byTitle = (left: AssetSummary, right: AssetSummary) => collator.compare(left.title, right.title);
  const byDate = (left: AssetSummary, right: AssetSummary) => Date.parse(left.created_at) - Date.parse(right.created_at);
  const byRating = (left: AssetSummary, right: AssetSummary, direction: 1 | -1) => {
    if (left.rating == null && right.rating == null) return byTitle(left, right);
    if (left.rating == null) return 1;
    if (right.rating == null) return -1;
    return (left.rating - right.rating) * direction || byDate(right, left) || byTitle(left, right);
  };

  return [...assets].sort((left, right) => {
    switch (sort) {
      case "oldest": return byDate(left, right) || byTitle(left, right);
      case "rating_high": return byRating(left, right, -1);
      case "rating_low": return byRating(left, right, 1);
      case "title": return byTitle(left, right);
      case "newest": return byDate(right, left) || byTitle(left, right);
    }
  });
}

export function matchesPrompt(prompt: PromptSummary, term?: string): boolean {
  const normalized = term?.trim().toLocaleLowerCase();
  if (!normalized) return true;
  return [
    prompt.title,
    prompt.intent,
    prompt.master_prompt,
    ...prompt.tags,
    ...prompt.variants.flatMap((variant) => [variant.model, variant.platform, variant.prompt]),
  ]
    .join(" ")
    .toLocaleLowerCase()
    .includes(normalized);
}
