import {
  type AppSettings,
  matchesAsset,
  matchesPrompt,
  sortAssets,
  type AssetPage,
  type AssetQuery,
  type AssetSummary,
  type AssetDetail,
  type CreatePromptInput,
  type HealthStatus,
  type ImportAssetOptions,
  type LibraryBootstrap,
  type LibraryClient,
  type PromptVariantInput,
  type PromptSummary,
  type SearchResult,
  type UpdateAppSettingsInput,
  type UpdateAssetContextInput,
  type UpdateAssetTagsInput,
} from "./library-client";

export const demoPrompts: PromptSummary[] = [
  {
    id: "prompt-rain-city",
    project_id: "project-city",
    title: "雨夜城市电影人像",
    intent: "建立具有克制霓虹和潮湿空气感的都市人物主视觉，重点保持人物自然与环境层次。",
    master_prompt: "雨夜的现代亚洲城市街角，一位穿深色风衣的年轻女性停在便利店暖光外，潮湿路面反射稀疏霓虹，电影级环境人像，真实皮肤质感，克制而安静。",
    status: "verified",
    tags: ["人像", "电影感", "雨夜"],
    updated_at: "2026-07-10T13:20:00Z",
    variants: [
      {
        id: "variant-rain-gpt",
        model: "GPT Image 2",
        platform: "OpenAI",
        prompt: "Create a restrained cinematic environmental portrait at a quiet Asian city corner after rain...",
        status: "verified",
        rating: 4.8,
        result_count: 18,
        parameter_summary: "4:5 · high fidelity",
        adaptation_note: "自然语言描述构图关系，减少风格词堆叠。",
      },
      {
        id: "variant-rain-seedream",
        model: "Seedream",
        platform: "ByteDance",
        prompt: "雨后城市街角电影感环境人像，便利店暖色侧光，路面少量霓虹反射……",
        status: "verified",
        rating: 4.6,
        result_count: 12,
        parameter_summary: "3:4 · 风格强度 0.35",
        adaptation_note: "中文主体与光线顺序更稳定，控制画面信息密度。",
      },
      {
        id: "variant-rain-nano",
        model: "Nano Banana 2",
        platform: "Google",
        prompt: "Using the reference character, place her outside a warm convenience store on a rainy city night...",
        status: "draft",
        rating: 4.2,
        result_count: 6,
        parameter_summary: "reference edit · 4:5",
        adaptation_note: "用于参考图编辑时明确保留人物身份和服饰结构。",
      },
    ],
  },
  {
    id: "prompt-skincare",
    project_id: "project-skincare",
    title: "高端护肤品晨光静物",
    intent: "为电商与品牌社媒生成留白充分、材质可信的护肤品主图。",
    master_prompt: "磨砂玻璃护肤品瓶置于浅米色天然石材上，清晨侧光穿过半透明亚麻，柔和阴影与充足留白，高端编辑感商业静物。",
    status: "verified",
    tags: ["商业静物", "护肤", "晨光"],
    updated_at: "2026-07-08T09:40:00Z",
    variants: [
      {
        id: "variant-skin-gpt",
        model: "GPT Image 2",
        platform: "OpenAI",
        prompt: "Premium skincare still life, frosted glass bottle on pale limestone, soft morning side light...",
        status: "verified",
        rating: 4.9,
        result_count: 24,
        parameter_summary: "1:1 · transparent label safe",
        adaptation_note: "写清标签区域和材质，避免额外装饰。",
      },
      {
        id: "variant-skin-seedream",
        model: "Seedream",
        platform: "ByteDance",
        prompt: "高端护肤品商业静物，磨砂玻璃瓶，浅色石灰岩台面，清晨柔和侧光……",
        status: "verified",
        rating: 4.7,
        result_count: 16,
        parameter_summary: "1:1 · 写实商业摄影",
        adaptation_note: "减少道具数量，使用材质与空间关系控制高级感。",
      },
    ],
  },
  {
    id: "prompt-orbit",
    project_id: "project-orbit",
    title: "轨道温室概念短片",
    intent: "建立一个从宁静外景推进到温室内部的六秒概念镜头。",
    master_prompt: "清晨地球轨道上的环形温室，镜头沿金属外壁缓慢推进，穿过玻璃进入漂浮植物之间，阳光产生柔和体积光。",
    status: "draft",
    tags: ["概念设计", "视频", "太空"],
    updated_at: "2026-07-06T16:10:00Z",
    variants: [
      {
        id: "variant-orbit-video",
        model: "Video model",
        platform: "Generic",
        prompt: "Six-second continuous dolly shot toward a circular orbital greenhouse at sunrise...",
        status: "draft",
        rating: 4.1,
        result_count: 4,
        parameter_summary: "6s · 16:9 · dolly in",
        adaptation_note: "拆分主体运动和镜头运动，明确结尾状态。",
      },
    ],
  },
];

function demoPreview(
  width: number,
  height: number,
  colors: [string, string, string],
): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
    <defs>
      <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
        <stop stop-color="${colors[0]}"/>
        <stop offset=".56" stop-color="${colors[1]}"/>
        <stop offset="1" stop-color="${colors[2]}"/>
      </linearGradient>
      <filter id="b"><feGaussianBlur stdDeviation="${Math.round(width * 0.04)}"/></filter>
    </defs>
    <rect width="100%" height="100%" fill="url(#g)"/>
    <circle cx="${Math.round(width * 0.72)}" cy="${Math.round(height * 0.22)}" r="${Math.round(width * 0.16)}" fill="#f5ddb0" opacity=".58" filter="url(#b)"/>
    <path d="M ${-width * 0.08} ${height * 0.82} Q ${width * 0.3} ${height * 0.5} ${width * 0.6} ${height * 0.78} T ${width * 1.08} ${height * 0.66} V ${height} H 0 Z" fill="#17231f" opacity=".62"/>
    <ellipse cx="${width * 0.24}" cy="${height * 0.7}" rx="${width * 0.16}" ry="${height * 0.13}" fill="#c47767" opacity=".5"/>
  </svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

export const demoAssets: AssetSummary[] = [
  {
    id: "asset-001",
    title: "便利店外的暖光停驻",
    media_type: "image",
    model: "GPT Image 2",
    prompt_title: "雨夜城市电影人像",
    project: "城市情绪研究",
    relationship: "output",
    rating: 5,
    tags: ["雨夜", "人像", "暖光"],
    created_at: "2026-07-10T12:42:00Z",
    aspect_ratio: "4:5",
    featured: true,
    palette: "nocturne",
    preview_url: demoPreview(960, 1200, ["#132828", "#4b5a59", "#725661"]),
  },
  {
    id: "asset-002",
    title: "石材台面的晨光瓶身",
    media_type: "image",
    model: "Seedream",
    prompt_title: "高端护肤品晨光静物",
    project: "护肤品牌提案",
    relationship: "output",
    rating: 5,
    tags: ["护肤", "静物", "留白"],
    created_at: "2026-07-09T08:17:00Z",
    aspect_ratio: "1:1",
    featured: true,
    palette: "limestone",
    preview_url: demoPreview(1024, 1024, ["#e4ddd0", "#aca693", "#767268"]),
  },
  {
    id: "asset-003",
    title: "轨道温室推进镜头",
    media_type: "video",
    model: "Video model",
    prompt_title: "轨道温室概念短片",
    project: "远未来片头",
    relationship: "output",
    rating: 4,
    tags: ["太空", "体积光", "运镜"],
    created_at: "2026-07-08T17:06:00Z",
    aspect_ratio: "16:9",
    duration_seconds: 6,
    palette: "orbit",
    preview_url: demoPreview(1280, 720, ["#071725", "#315363", "#896d53"]),
  },
  {
    id: "asset-004",
    title: "粗粝织物与陶土色谱",
    media_type: "reference",
    model: null,
    prompt_title: null,
    project: "材质档案",
    relationship: "reference",
    rating: 4,
    tags: ["材质", "陶土", "色板"],
    created_at: "2026-07-07T10:22:00Z",
    aspect_ratio: "3:2",
    palette: "terracotta",
    preview_url: demoPreview(1200, 800, ["#9d4e39", "#d48e69", "#5a3d35"]),
  },
  {
    id: "asset-005",
    title: "雨幕中的低机位侧影",
    media_type: "image",
    model: "Seedream",
    prompt_title: "雨夜城市电影人像",
    project: "城市情绪研究",
    relationship: "derived",
    rating: 4,
    tags: ["雨夜", "侧影", "低机位"],
    created_at: "2026-07-06T14:38:00Z",
    aspect_ratio: "3:4",
    palette: "afterrain",
    preview_url: demoPreview(900, 1200, ["#233f49", "#607a7c", "#202932"]),
  },
  {
    id: "asset-006",
    title: "玻璃折射实验 B",
    media_type: "image",
    model: "Nano Banana 2",
    prompt_title: "高端护肤品晨光静物",
    project: "护肤品牌提案",
    relationship: "output",
    rating: 3,
    tags: ["玻璃", "折射", "实验"],
    created_at: "2026-07-05T11:03:00Z",
    aspect_ratio: "1:1",
    palette: "glass",
    preview_url: demoPreview(1024, 1024, ["#d8e4df", "#8ba3a8", "#ecddd0"]),
  },
  {
    id: "asset-007",
    title: "温室内部植物漂浮测试",
    media_type: "video",
    model: "Video model",
    prompt_title: "轨道温室概念短片",
    project: "远未来片头",
    relationship: "output",
    rating: 3,
    tags: ["植物", "漂浮", "测试"],
    created_at: "2026-07-04T19:15:00Z",
    aspect_ratio: "16:9",
    duration_seconds: 5,
    palette: "greenhouse",
    preview_url: demoPreview(1280, 720, ["#405d46", "#8a9a72", "#2d4135"]),
  },
  {
    id: "asset-008",
    title: "晨雾山谷的层次参考",
    media_type: "reference",
    model: null,
    prompt_title: null,
    project: null,
    relationship: "inspiration",
    rating: null,
    tags: ["晨雾", "空间层次", "自然"],
    created_at: "2026-07-02T06:50:00Z",
    aspect_ratio: "3:2",
    palette: "mist",
    preview_url: demoPreview(1200, 800, ["#d9dfd6", "#809486", "#284035"]),
  },
];

const bootstrap: LibraryBootstrap = {
  library_name: "Demo Creative Library",
  initialized: true,
  storage_mode: "demo",
  counts: { assets: demoAssets.length, prompts: demoPrompts.length, projects: 4, uncategorized: 1 },
  models: ["GPT Image 2", "Seedream", "Nano Banana 2", "Video model"],
  projects: [
    { id: "project-city", name: "城市情绪研究", description: "都市环境、人物与电影感光线实验。", updated_at: "2026-07-10T13:20:00Z" },
    { id: "project-skincare", name: "护肤品牌提案", description: "电商主图与品牌社媒视觉方向。", updated_at: "2026-07-09T08:17:00Z" },
    { id: "project-orbit", name: "远未来片头", description: "轨道空间与未来生态概念短片。", updated_at: "2026-07-08T17:06:00Z" },
    { id: "project-material", name: "材质档案", description: "可复用的色彩、织物与表面参考。", updated_at: "2026-07-07T10:22:00Z" },
  ],
  prompts: demoPrompts,
};

export class MockLibraryClient implements LibraryClient {
  readonly mode = "mock" as const;

  async health(): Promise<HealthStatus> {
    return { status: "ok", version: "demo", storage: "in-memory", database: "example data" };
  }

  async getAppSettings(): Promise<AppSettings> {
    return {
      config_version: 1,
      config_path: "浏览器示例不使用配置文件",
      ui: {
        default_asset_view: "card",
        sidebar_collapsed: false,
        sidebar_width: 238,
      },
      storage: {
        backend: "demo",
        source: "demo",
        active_data_root: "浏览器内存示例",
        configured_data_root: "浏览器内存示例",
        database_path: "无本地数据库",
        media_path: "示例资源",
        exports_path: "不可用",
        temp_path: "不可用",
        restart_required: false,
      },
    };
  }

  async updateAppSettings(input: UpdateAppSettingsInput): Promise<AppSettings> {
    const settings = await this.getAppSettings();
    return { ...settings, ui: input.ui };
  }

  async chooseDataRoot(): Promise<AppSettings | null> {
    return null;
  }

  async bootstrap(): Promise<LibraryBootstrap> {
    return bootstrap;
  }

  async listAssets(query: AssetQuery = {}): Promise<AssetPage> {
    const items = sortAssets(demoAssets.filter((asset) => matchesAsset(asset, query)), query.sort);
    return { items: items.slice(0, query.limit ?? items.length), total: items.length };
  }

  async createProject(_input: { name: string; description?: string }): Promise<void> {
    throw new Error("浏览器示例为只读，请在 Tauri 桌面端创建项目");
  }

  async createPrompt(_input: CreatePromptInput): Promise<void> {
    throw new Error("浏览器示例为只读，请在 Tauri 桌面端创建 Prompt");
  }

  async addPromptVariant(_promptId: string, _input: PromptVariantInput): Promise<void> {
    throw new Error("浏览器示例为只读，请在 Tauri 桌面端添加模型变体");
  }

  async importAssets(_files: File[], _options?: ImportAssetOptions): Promise<AssetSummary[]> {
    return [];
  }

  async getAssetDetail(assetId: string): Promise<AssetDetail> {
    const asset = demoAssets.find((item) => item.id === assetId);
    if (!asset) throw new Error("未找到素材");
    const prompt = demoPrompts.find((item) => item.title === asset.prompt_title);
    const variant = prompt?.variants.find((item) => item.model === asset.model);
    return {
      id: asset.id,
      original_url: asset.media_type === "video" ? null : asset.preview_url,
      original_filename: asset.title,
      created_at: asset.created_at,
      relation_type: asset.relationship,
      prompt: prompt ? {
        id: prompt.id,
        project_id: prompt.project_id,
        title: prompt.title,
        creative_intent: prompt.intent,
        base_prompt: prompt.master_prompt,
      } : null,
      variant: variant ? {
        id: variant.id,
        provider: variant.platform,
        model: variant.model,
        prompt_text: variant.prompt,
        parameters: {},
        adaptation_notes: variant.adaptation_note,
      } : null,
      generation: variant ? {
        id: `run-${asset.id}`,
        source: "manual",
        status: "succeeded",
        prompt_text_snapshot: variant.prompt,
        parameters_snapshot: {},
      } : null,
      evaluation: asset.rating ? {
        overall_score: asset.rating,
        is_featured: Boolean(asset.featured),
        is_reusable: true,
        lesson: "示例详情：真实桌面端会读取持久化的生成快照与复盘。",
      } : null,
      related_asset_count: 0,
      tags: asset.tags,
    };
  }

  async updateAssetContext(_input: UpdateAssetContextInput): Promise<AssetDetail> {
    throw new Error("浏览器示例为只读，请在 Tauri 桌面端编辑素材资料");
  }

  async updateAssetTags(_input: UpdateAssetTagsInput): Promise<AssetDetail> {
    throw new Error("浏览器示例为只读，请在 Tauri 桌面端编辑素材标签");
  }

  async searchLibrary(query: AssetQuery): Promise<SearchResult> {
    const assets = await this.listAssets(query);
    return {
      ...assets,
      prompts: demoPrompts.filter((prompt) => matchesPrompt(prompt, query.query)),
    };
  }
}
