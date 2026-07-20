import { invoke } from "@tauri-apps/api/core";
import {
  emptyBootstrap,
  matchesAsset,
  sortAssets,
  type AppSettings,
  type AssetDetail,
  type AssetPage,
  type AssetQuery,
  type AssetSummary,
  type CreatePromptInput,
  type HealthStatus,
  type ImportAssetOptions,
  type LibraryBootstrap,
  type LibraryClient,
  type PromptVariantInput,
  type SearchResult,
  type UpdateAppSettingsInput,
  type UpdateAssetContextInput,
  type UpdateAssetTagsInput,
} from "./library-client";

type AssetResponse = AssetSummary[] | AssetPage;

function asPage(response: AssetResponse): AssetPage {
  return Array.isArray(response)
    ? { items: response, total: response.length }
    : { items: response.items ?? [], total: response.total ?? response.items?.length ?? 0 };
}

function normalizeBootstrap(response: Partial<LibraryBootstrap> | null): LibraryBootstrap {
  const fallback = emptyBootstrap("local");
  if (!response) return fallback;
  return {
    ...fallback,
    ...response,
    counts: { ...fallback.counts, ...response.counts },
    models: response.models ?? [],
    projects: response.projects ?? [],
    prompts: response.prompts ?? [],
  };
}

export class TauriLibraryClient implements LibraryClient {
  readonly mode = "tauri" as const;

  health(): Promise<HealthStatus> {
    return invoke<HealthStatus>("health");
  }

  getAppSettings(): Promise<AppSettings> {
    return invoke<AppSettings>("get_app_settings");
  }

  updateAppSettings(input: UpdateAppSettingsInput): Promise<AppSettings> {
    return invoke<AppSettings>("update_app_settings", { request: input });
  }

  chooseDataRoot(): Promise<AppSettings | null> {
    return invoke<AppSettings | null>("choose_data_root");
  }

  async bootstrap(): Promise<LibraryBootstrap> {
    return normalizeBootstrap(await invoke<Partial<LibraryBootstrap>>("bootstrap_library"));
  }

  async listAssets(query: AssetQuery = {}): Promise<AssetPage> {
    const page = asPage(await invoke<AssetResponse>("list_assets"));
    const items = sortAssets(page.items.filter((asset) => matchesAsset(asset, query)), query.sort);
    return { items: items.slice(0, query.limit ?? items.length), total: items.length };
  }

  async searchLibrary(query: AssetQuery): Promise<SearchResult> {
    const response = await invoke<AssetResponse>("search_library", { query: query.query ?? "" });
    const page = asPage(response);
    const items = sortAssets(
      page.items.filter((asset) => matchesAsset(asset, { ...query, query: undefined })),
      query.sort,
    );
    return { items: items.slice(0, query.limit ?? items.length), total: items.length, prompts: [] };
  }

  async createProject(input: { name: string; description?: string }): Promise<void> {
    await invoke("create_project", {
      request: {
        name: input.name,
        description: input.description || null,
      },
    });
  }

  async createPrompt(input: CreatePromptInput): Promise<void> {
    await invoke("create_prompt", {
      request: {
        project_id: input.project_id,
        title: input.title,
        creative_intent: input.creative_intent,
        base_prompt: input.base_prompt,
        negative_constraints: input.negative_constraints || null,
        initial_variant: input.initial_variant ? normalizeVariantDraft(input.initial_variant) : null,
      },
    });
  }

  async addPromptVariant(promptId: string, input: PromptVariantInput): Promise<void> {
    await invoke("create_prompt_variant", {
      request: {
        prompt_id: promptId,
        draft: normalizeVariantDraft(input),
      },
    });
  }

  async importAssets(files: File[], options: ImportAssetOptions = {}): Promise<AssetSummary[]> {
    const imported: AssetSummary[] = [];
    for (const file of files) {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      imported.push(await invoke<AssetSummary>("import_asset_bytes", {
        request: {
          filename: file.name,
          mimeType: file.type || "application/octet-stream",
          bytes,
        },
      }));
    }
    if (options.variant_id && imported.length > 0) {
      const generation = await invoke<{ id: string }>("record_generation", {
        request: {
          variant_id: options.variant_id,
          source: "manual",
          prompt_text_snapshot: options.prompt_text || "Imported generation result",
          input_extra: {},
          parameters_snapshot: options.parameters ?? {},
          external_url: null,
          cost: null,
          output_asset_ids: imported.map((asset) => asset.id),
          reference_asset_ids: [],
          outcome: { kind: "succeeded" },
        },
      });
      const rating = options.rating;
      if (rating) {
        await Promise.all(imported.map((asset) => invoke("create_evaluation", {
          request: {
            asset_id: asset.id,
            generation_run_id: generation.id,
            overall_score: rating,
            is_featured: Boolean(options.featured),
            is_reusable: rating >= 4,
            dimension_scores: {},
            strengths: null,
            issues: null,
            lesson: options.lesson || null,
          },
        })));
      }
    }
    return imported;
  }

  getAssetDetail(assetId: string): Promise<AssetDetail> {
    return invoke<AssetDetail>("get_asset_detail", { assetId });
  }

  updateAssetContext(input: UpdateAssetContextInput): Promise<AssetDetail> {
    return invoke<AssetDetail>("update_asset_context", { request: input });
  }

  updateAssetTags(input: UpdateAssetTagsInput): Promise<AssetDetail> {
    return invoke<AssetDetail>("update_asset_tags", { request: input });
  }
}

function normalizeVariantDraft(input: PromptVariantInput) {
  return {
    provider: input.provider,
    model: input.model,
    model_version: input.model_version || null,
    prompt_text: input.prompt_text,
    parameters: input.parameters ?? {},
    adaptation_notes: input.adaptation_notes || null,
    success_tips: input.success_tips || null,
    failure_patterns: input.failure_patterns || null,
  };
}
