import { useEffect, useMemo, useRef, useState, type CSSProperties, type FormEvent } from "react";
import type {
  AssetDetail,
  AssetQuery,
  AssetRelationship,
  AssetSort,
  AssetSummary,
  MediaType,
  PromptSummary,
  UpdateAssetContextInput,
  UpdateAssetTagsInput,
} from "../lib/library-client";
import { Icon } from "./Icon";
import { SelectMenu, type SelectMenuOption } from "./SelectMenu";

interface AssetLibraryProps {
  assets: AssetSummary[];
  total: number;
  models: string[];
  query: AssetQuery;
  loading: boolean;
  refreshing: boolean;
  displayMode: DisplayMode;
  isDemo: boolean;
  prompts: PromptSummary[];
  availableTags: string[];
  writing: boolean;
  onQueryChange: (query: AssetQuery) => void;
  onDisplayModeChange: (mode: DisplayMode) => void;
  onImport: () => void;
  onLoadDetail: (assetId: string) => Promise<AssetDetail>;
  onUpdateAssetContext: (input: UpdateAssetContextInput) => Promise<AssetDetail>;
  onUpdateAssetTags: (input: UpdateAssetTagsInput) => Promise<AssetDetail>;
}

export type DisplayMode = "card" | "list";

const mediaFilters: Array<{ value: MediaType | "all"; label: string }> = [
  { value: "all", label: "全部" },
  { value: "image", label: "图片" },
  { value: "video", label: "视频" },
  { value: "reference", label: "参考素材" },
];

const sortOptions: Array<SelectMenuOption<AssetSort>> = [
  { value: "newest", label: "最近添加", description: "按创建时间从新到旧" },
  { value: "oldest", label: "最早添加", description: "按创建时间从旧到新" },
  { value: "rating_high", label: "评分最高", description: "优先展示高质量结果" },
  { value: "rating_low", label: "评分最低", description: "适合回看待改进结果" },
  { value: "title", label: "名称顺序", description: "按中文与字母顺序排列" },
];

const relationshipLabels: Record<AssetRelationship, string> = {
  output: "生成结果",
  reference: "参考输入",
  inspiration: "灵感收藏",
  derived: "衍生素材",
};

const mediaLabels: Record<MediaType, string> = {
  image: "图片",
  video: "视频",
  reference: "参考",
};

function TagFilterMenu({
  tags,
  selected,
  onChange,
}: {
  tags: string[];
  selected: string[];
  onChange: (tags: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const options = useMemo(() => {
    const values = new Map<string, string>();
    for (const tag of [...selected, ...tags]) values.set(tag.toLocaleLowerCase(), tag);
    return [...values.values()].sort((left, right) => left.localeCompare(right, "zh-CN"));
  }, [selected, tags]);
  const visibleOptions = options.filter((tag) => tag.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));

  useEffect(() => {
    if (!open) return undefined;
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const toggle = (tag: string) => {
    const active = selected.some((value) => value.toLocaleLowerCase() === tag.toLocaleLowerCase());
    onChange(active
      ? selected.filter((value) => value.toLocaleLowerCase() !== tag.toLocaleLowerCase())
      : [...selected, tag]);
  };

  return (
    <div className="tag-filter-menu" ref={rootRef}>
      <button
        className={`filter-chip tag-filter-trigger ${selected.length > 0 ? "is-active" : ""}`}
        type="button"
        disabled={options.length === 0}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <Icon name="tag" size={14} />
        {selected.length ? `标签 · ${selected.length}` : "标签"}
        <Icon name="chevron" size={11} />
      </button>
      {open && (
        <div className="tag-filter-popover">
          <header>
            <div><strong>按标签筛选</strong><small>多选时需同时匹配</small></div>
            {selected.length > 0 && <button type="button" onClick={() => onChange([])}>清除</button>}
          </header>
          {options.length > 7 && (
            <label className="tag-filter-search">
              <Icon name="search" size={13} />
              <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索标签…" autoFocus />
            </label>
          )}
          <div className="tag-filter-options">
            {visibleOptions.map((tag) => {
              const active = selected.some((value) => value.toLocaleLowerCase() === tag.toLocaleLowerCase());
              return (
                <button type="button" className={active ? "is-active" : ""} key={tag} onClick={() => toggle(tag)}>
                  <span>{active && <Icon name="check" size={12} />}</span>
                  <strong>#{tag}</strong>
                </button>
              );
            })}
            {visibleOptions.length === 0 && <p>没有匹配的标签</p>}
          </div>
        </div>
      )}
    </div>
  );
}

function formatAspectRatio(value?: string | null): CSSProperties["aspectRatio"] {
  const [width, height] = value?.split(":").map(Number) ?? [];
  return width && height ? `${width} / ${height}` : "4 / 3";
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", { month: "short", day: "numeric" }).format(new Date(value));
}

function formatDuration(seconds?: number | null): string | null {
  if (seconds == null) return null;
  const mins = Math.floor(seconds / 60);
  const secs = Math.round(seconds % 60).toString().padStart(2, "0");
  return `${mins}:${secs}`;
}

function paletteClass(palette?: string): string {
  return `palette-${(palette ?? "default").replace(/[^a-z0-9-]/gi, "")}`;
}

function AssetArtwork({
  asset,
  mode = "card",
  aspectRatio,
}: {
  asset: AssetSummary;
  mode?: "card" | "list" | "detail";
  aspectRatio?: CSSProperties["aspectRatio"];
}) {
  const duration = formatDuration(asset.duration_seconds);
  const fallbackAspectRatio = aspectRatio ?? formatAspectRatio(asset.aspect_ratio);
  return (
    <div
      className={`asset-art ${paletteClass(asset.palette)} is-${mode}`}
      style={mode === "list"
        ? { aspectRatio: "16 / 10" }
        : mode === "detail" || !asset.preview_url
          ? { aspectRatio: fallbackAspectRatio }
          : undefined}
      aria-label={`${asset.title} 的预览`}
    >
      {asset.preview_url && (
        <>
          <img className="asset-art-backdrop" src={asset.preview_url} alt="" aria-hidden="true" />
          <img
            className="asset-art-image"
            src={asset.preview_url}
            alt=""
            loading="lazy"
            decoding="async"
          />
        </>
      )}
      <div className="art-glow" />
      <div className="art-plane one" />
      <div className="art-plane two" />
      <div className="art-grain" />
      {asset.media_type === "video" && (
        <span className="play-button"><span /></span>
      )}
      <span className="media-pill">
        <Icon name={asset.media_type === "video" ? "video" : asset.media_type === "reference" ? "reference" : "image"} size={13} />
        {duration ?? mediaLabels[asset.media_type]}
      </span>
      {asset.featured && <span className="featured-pill"><Icon name="star" size={12} /> 精选</span>}
    </div>
  );
}

function AssetCard({ asset, onOpen }: { asset: AssetSummary; onOpen: () => void }) {
  return (
    <article className="asset-card" onClick={onOpen} onKeyDown={(event) => event.key === "Enter" && onOpen()} tabIndex={0}>
      <AssetArtwork asset={asset} />
      <div className="asset-card-body">
        <div className="asset-card-title-row">
          <h3>{asset.title}</h3>
          <button className="quiet-icon" type="button" aria-label="更多操作" onClick={(event) => event.stopPropagation()}>
            <Icon name="more" size={17} />
          </button>
        </div>
        <p>{asset.prompt_title ?? (asset.relationship ? relationshipLabels[asset.relationship] : "独立素材")}</p>
        <div className="asset-meta-row">
          <span className="model-mark" />
          <span>{asset.model ?? asset.project ?? "本地导入"}</span>
          <span className="meta-spacer" />
          {asset.rating != null && <span className="rating"><Icon name="star" size={12} /> {asset.rating.toFixed(1)}</span>}
          <time>{formatDate(asset.created_at)}</time>
        </div>
      </div>
    </article>
  );
}

function AssetListRow({ asset, onOpen }: { asset: AssetSummary; onOpen: () => void }) {
  return (
    <article
      className="asset-list-row"
      onClick={onOpen}
      onKeyDown={(event) => event.key === "Enter" && onOpen()}
      tabIndex={0}
    >
      <AssetArtwork asset={asset} mode="list" />
      <div className="asset-list-primary">
        <div className="asset-card-title-row">
          <h3>{asset.title}</h3>
        </div>
        <p>{asset.prompt_title ?? (asset.relationship ? relationshipLabels[asset.relationship] : "独立素材")}</p>
        <div className="asset-list-tags">
          {asset.tags.slice(0, 3).map((tag) => <span key={tag}>#{tag}</span>)}
        </div>
        <div className="asset-list-compact-meta">
          <span>{asset.model ?? "本地导入"}</span>
          <span>{asset.project ?? "未归档"}</span>
          <span>{asset.rating != null ? `${asset.rating.toFixed(1)} 分` : "待评价"}</span>
        </div>
      </div>
      <div className="asset-list-cell asset-list-model"><span>模型</span><strong>{asset.model ?? "本地导入"}</strong></div>
      <div className="asset-list-cell asset-list-project"><span>项目</span><strong>{asset.project ?? "未归档"}</strong></div>
      <div className="asset-list-cell asset-list-type"><span>类型</span><strong>{mediaLabels[asset.media_type]}</strong></div>
      <div className="asset-list-cell asset-list-score">
        <span>评分</span>
        <strong>{asset.rating != null ? <><Icon name="star" size={12} /> {asset.rating.toFixed(1)}</> : "待评价"}</strong>
      </div>
      <time className="asset-list-date" dateTime={asset.created_at}>{formatDate(asset.created_at)}</time>
      <button className="quiet-icon" type="button" aria-label="更多操作" onClick={(event) => event.stopPropagation()}>
        <Icon name="more" size={17} />
      </button>
    </article>
  );
}

function AssetSkeleton() {
  return (
    <div className="asset-card skeleton-card" aria-hidden="true">
      <div className="skeleton-block" />
      <div className="asset-card-body">
        <span className="skeleton-line wide" />
        <span className="skeleton-line" />
        <span className="skeleton-line short" />
      </div>
    </div>
  );
}

function AssetListSkeleton() {
  return (
    <div className="asset-list-row skeleton-list-row" aria-hidden="true">
      <div className="skeleton-list-art skeleton-block" />
      <div className="asset-list-primary">
        <span className="skeleton-line wide" />
        <span className="skeleton-line" />
      </div>
      <span className="skeleton-line" />
      <span className="skeleton-line" />
      <span className="skeleton-line short" />
    </div>
  );
}

function EmptyLibrary({ filtered, onReset, onImport }: { filtered: boolean; onReset: () => void; onImport: () => void }) {
  return (
    <div className="empty-state">
      <div className="empty-orbit">
        <span className="empty-icon"><Icon name={filtered ? "search" : "image"} size={25} /></span>
      </div>
      <h2>{filtered ? "没有找到相符的素材" : "从第一份创作结果开始"}</h2>
      <p>
        {filtered
          ? "换一个关键词或放宽筛选条件。低分与未整理素材也可能藏着有用线索。"
          : "导入图片或视频，再补充它所用的 Prompt、模型和参数。关系可以稍后慢慢完善。"}
      </p>
      <button className="button primary" onClick={filtered ? onReset : onImport}>
        <Icon name={filtered ? "close" : "upload"} size={16} />
        {filtered ? "清除筛选" : "导入第一份素材"}
      </button>
      {!filtered && <small>支持 PNG、JPEG、WebP、AVIF 与常见视频格式</small>}
    </div>
  );
}

function OriginalPreview({
  asset,
  url,
  dimensions,
  onClose,
}: {
  asset: AssetSummary;
  url: string;
  dimensions: string;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="original-preview-layer" role="dialog" aria-modal="true" aria-label={`${asset.title} 原件预览`}>
      <button className="original-preview-scrim" type="button" onClick={onClose} aria-label="关闭原件预览" />
      <header>
        <div>
          <span className="eyebrow">原件预览</span>
          <strong>{asset.title}</strong>
          <small>{dimensions}</small>
        </div>
        <button className="preview-close" type="button" onClick={onClose} aria-label="关闭原件预览"><Icon name="close" size={19} /></button>
      </header>
      <figure>
        {asset.media_type === "video"
          ? <video src={url} controls autoPlay={false} />
          : <img src={url} alt={asset.title} />}
      </figure>
      <div className="preview-hint">Esc 关闭 · 保持原始比例完整显示</div>
    </div>
  );
}

function AssetDrawer({
  asset,
  prompts,
  availableTags,
  writing,
  isDemo,
  onClose,
  onLoadDetail,
  onUpdateAssetContext,
  onUpdateAssetTags,
}: {
  asset: AssetSummary;
  prompts: PromptSummary[];
  availableTags: string[];
  writing: boolean;
  isDemo: boolean;
  onClose: () => void;
  onLoadDetail: (assetId: string) => Promise<AssetDetail>;
  onUpdateAssetContext: (input: UpdateAssetContextInput) => Promise<AssetDetail>;
  onUpdateAssetTags: (input: UpdateAssetTagsInput) => Promise<AssetDetail>;
}) {
  const [detail, setDetail] = useState<AssetDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [variantId, setVariantId] = useState("");
  const [promptDraft, setPromptDraft] = useState("");
  const [parametersDraft, setParametersDraft] = useState("{}");
  const [ratingDraft, setRatingDraft] = useState<number | undefined>();
  const [featuredDraft, setFeaturedDraft] = useState(false);
  const [lessonDraft, setLessonDraft] = useState("");
  const [editError, setEditError] = useState<string | null>(null);
  const [editingTags, setEditingTags] = useState(false);
  const [tagDraft, setTagDraft] = useState<string[]>([]);
  const [tagInput, setTagInput] = useState("");
  const [tagError, setTagError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    void onLoadDetail(asset.id)
      .then((value) => {
        if (active) setDetail(value);
      })
      .catch((reason) => {
        if (active) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => { active = false; };
  }, [asset.id, onLoadDetail]);

  const variants = useMemo(
    () => prompts.flatMap((prompt) => prompt.variants.map((variant) => ({ prompt, variant }))),
    [prompts],
  );
  const variantOptions: Array<SelectMenuOption<string>> = [
    { value: "", label: "不关联 Prompt", description: "保留为独立素材，仅保存评价资料" },
    ...variants.map(({ prompt, variant }) => ({
      value: variant.id,
      label: `${variant.model} · ${prompt.title}`,
      description: variant.platform,
    })),
  ];
  const promptText = detail?.generation?.prompt_text_snapshot ?? detail?.variant?.prompt_text ?? detail?.prompt?.base_prompt;
  const rating = detail?.evaluation?.overall_score ?? asset.rating;
  const relationship = detail?.relation_type ?? asset.relationship;
  const dimensions = detail?.width && detail?.height ? `${detail.width} × ${detail.height}` : asset.aspect_ratio ?? "—";
  const detailAspectRatio = detail?.width && detail?.height
    ? `${detail.width} / ${detail.height}`
    : formatAspectRatio(asset.aspect_ratio);
  const originalUrl = detail?.original_url ?? asset.preview_url ?? null;
  const displayTags = detail?.tags ?? asset.tags;
  const copyPrompt = () => {
    if (promptText) void navigator.clipboard?.writeText(promptText);
  };
  const startEditing = () => {
    setVariantId(detail?.variant?.id ?? "");
    setPromptDraft(promptText ?? "");
    setParametersDraft(JSON.stringify(
      detail?.generation?.parameters_snapshot ?? detail?.variant?.parameters ?? {},
      null,
      2,
    ));
    setRatingDraft(rating ?? undefined);
    setFeaturedDraft(Boolean(detail?.evaluation?.is_featured ?? asset.featured));
    setLessonDraft(detail?.evaluation?.lesson ?? "");
    setEditError(null);
    setEditing(true);
  };
  const chooseVariant = (nextVariantId: string) => {
    setVariantId(nextVariantId);
    const selected = variants.find(({ variant }) => variant.id === nextVariantId);
    if (selected) {
      setPromptDraft(selected.variant.prompt);
      setParametersDraft("{}");
    } else {
      setPromptDraft("");
      setParametersDraft("{}");
    }
  };
  const saveContext = async (event: FormEvent) => {
    event.preventDefault();
    setEditError(null);
    if (variantId && !promptDraft.trim()) {
      setEditError("关联模型变体后需要填写实际使用的 Prompt");
      return;
    }
    if (!ratingDraft && (featuredDraft || lessonDraft.trim())) {
      setEditError("设置精选或复盘前，请先选择 1–5 分");
      return;
    }
    let parameters: Record<string, unknown> = {};
    try {
      const parsed = parametersDraft.trim() ? JSON.parse(parametersDraft) as unknown : {};
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
        throw new Error("参数必须是 JSON 对象");
      }
      parameters = parsed as Record<string, unknown>;
    } catch (reason) {
      setEditError(reason instanceof Error ? reason.message : "参数 JSON 格式无效");
      return;
    }
    try {
      const updated = await onUpdateAssetContext({
        asset_id: asset.id,
        variant_id: variantId || undefined,
        prompt_text_snapshot: variantId ? promptDraft.trim() : undefined,
        parameters_snapshot: parameters,
        overall_score: ratingDraft,
        is_featured: featuredDraft,
        lesson: lessonDraft.trim() || undefined,
      });
      setDetail(updated);
      setEditing(false);
    } catch (reason) {
      setEditError(reason instanceof Error ? reason.message : String(reason));
    }
  };
  const startTagEditing = () => {
    setTagDraft(displayTags);
    setTagInput("");
    setTagError(null);
    setEditingTags(true);
  };
  const addTags = (rawValue: string) => {
    const incoming = rawValue
      .split(/[,，\n]/)
      .map((value) => value.trim().replace(/^[#＃]+/, "").trim())
      .filter(Boolean);
    if (incoming.some((tag) => [...tag].length > 32)) {
      setTagError("单个标签最多 32 个字符");
      return;
    }
    const next = [...tagDraft];
    for (const tag of incoming) {
      if (!next.some((value) => value.toLocaleLowerCase() === tag.toLocaleLowerCase())) next.push(tag);
    }
    if (next.length > 20) {
      setTagError("每份素材最多添加 20 个标签");
      return;
    }
    setTagDraft(next);
    setTagInput("");
    setTagError(null);
  };
  const saveTags = async (event: FormEvent) => {
    event.preventDefault();
    const nextTags = [...tagDraft];
    for (const tag of tagInput
      .split(/[,，\n]/)
      .map((value) => value.trim().replace(/^[#＃]+/, "").trim())
      .filter(Boolean)) {
      if (!nextTags.some((value) => value.toLocaleLowerCase() === tag.toLocaleLowerCase())) nextTags.push(tag);
    }
    if (nextTags.length > 20) {
      setTagError("每份素材最多添加 20 个标签");
      return;
    }
    if (nextTags.some((tag) => [...tag].length > 32)) {
      setTagError("单个标签最多 32 个字符");
      return;
    }
    setTagError(null);
    try {
      const updated = await onUpdateAssetTags({ asset_id: asset.id, tags: nextTags });
      setDetail(updated);
      setEditingTags(false);
      setTagInput("");
    } catch (reason) {
      setTagError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return (
    <div className="drawer-layer" role="dialog" aria-modal="true" aria-label={asset.title}>
      <button className="drawer-scrim" onClick={onClose} aria-label="关闭素材详情" />
      <aside className="asset-drawer">
        <header>
          <div>
            <span className="eyebrow">素材详情</span>
            <h2>{asset.title}</h2>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭素材详情"><Icon name="close" /></button>
        </header>

        <button
          className="drawer-preview"
          type="button"
          disabled={!originalUrl}
          onClick={() => originalUrl && setPreviewOpen(true)}
          aria-label={originalUrl ? "打开原件预览" : "原件暂不可预览"}
        >
          <AssetArtwork asset={asset} mode="detail" aspectRatio={detailAspectRatio} />
          {originalUrl && <span className="drawer-preview-action"><Icon name="expand" size={14} /> 查看原件</span>}
        </button>

        <section className="drawer-section">
          <div className="detail-grid">
            <div><span>类型</span><strong>{relationship ? relationshipLabels[relationship] : mediaLabels[asset.media_type]}</strong></div>
            <div><span>尺寸</span><strong>{dimensions}</strong></div>
            <div><span>模型</span><strong>{detail?.variant?.model ?? asset.model ?? "本地导入"}</strong></div>
            <div><span>评分</span><strong>{rating ? `${rating.toFixed(1)} / 5` : "待评价"}</strong></div>
          </div>
        </section>

        <section className="drawer-section lineage-section">
          <div className="section-heading"><div><span className="eyebrow">生成链路</span><h3>从结果回到方法</h3></div></div>
          <div className="lineage">
            <span>{asset.project ?? "未归档项目"}</span><Icon name="chevron" size={13} />
            <span>{detail?.prompt?.title ?? asset.prompt_title ?? "独立参考素材"}</span><Icon name="chevron" size={13} />
            <span>{detail?.variant?.model ?? asset.model ?? "原始文件"}</span>
          </div>
          {loading && <div className="detail-loading"><span /> 正在读取真实链路…</div>}
          {error && <div className="detail-error">{error}</div>}
        </section>

        {editing && (
          <section className="drawer-section asset-context-editor">
            <div className="section-heading">
              <div><span className="eyebrow">整理资料</span><h3>补充 Prompt 与创作判断</h3></div>
              <button className="copy-button" type="button" onClick={() => setEditing(false)}>取消</button>
            </div>
            <form onSubmit={(event) => void saveContext(event)}>
              <label>
                <span>关联模型变体</span>
                <SelectMenu
                  value={variantId}
                  options={variantOptions}
                  label="选择模型变体"
                  icon="sparkles"
                  onChange={chooseVariant}
                  className="context-variant-select"
                />
              </label>
              {variantId && (
                <>
                  <label>
                    <span>实际使用的 Prompt</span>
                    <textarea value={promptDraft} onChange={(event) => setPromptDraft(event.target.value)} rows={6} />
                  </label>
                  <label>
                    <span>参数 JSON</span>
                    <textarea value={parametersDraft} onChange={(event) => setParametersDraft(event.target.value)} rows={3} spellCheck={false} />
                  </label>
                </>
              )}
              <div className="context-rating-field">
                <span>评分</span>
                <div>
                  {[1, 2, 3, 4, 5].map((score) => (
                    <button
                      type="button"
                      key={score}
                      className={ratingDraft === score ? "is-active" : ""}
                      onClick={() => {
                        const next = ratingDraft === score ? undefined : score;
                        setRatingDraft(next);
                        if (!next) setFeaturedDraft(false);
                      }}
                    ><Icon name="star" size={12} /> {score}</button>
                  ))}
                </div>
              </div>
              <label className="context-featured-field">
                <input
                  type="checkbox"
                  checked={featuredDraft}
                  disabled={!ratingDraft}
                  onChange={(event) => setFeaturedDraft(event.target.checked)}
                />
                <span><strong>标记为精选</strong><small>需要先设置评分</small></span>
              </label>
              <label>
                <span>一句复盘</span>
                <input value={lessonDraft} onChange={(event) => setLessonDraft(event.target.value)} placeholder="为什么值得保留，或下次应如何调整？" />
              </label>
              {isDemo && <p className="context-demo-note">浏览器示例可体验编辑界面，实际保存请使用 Tauri 桌面端。</p>}
              {editError && <p className="context-edit-error">{editError}</p>}
              <footer>
                <button className="button" type="button" onClick={() => setEditing(false)}>取消</button>
                <button className="button primary" disabled={writing || isDemo}>
                  {writing ? "保存中…" : isDemo ? "桌面端可保存" : "保存资料"}
                </button>
              </footer>
            </form>
          </section>
        )}

        {!editing && promptText && (
          <section className="drawer-section prompt-snapshot">
            <div className="section-heading">
              <div><span className="eyebrow">实际生成快照</span><h3>当时真正使用的 Prompt</h3></div>
              <div className="section-heading-actions">
                <button className="copy-button" onClick={startEditing}><Icon name="edit" size={13} /> 编辑资料</button>
                <button className="copy-button" onClick={copyPrompt}><Icon name="copy" size={13} /> 复制</button>
              </div>
            </div>
            <p>{promptText}</p>
            {detail?.generation && (
              <div className="snapshot-meta">
                <span>{detail.generation.source}</span>
                <span>{detail.generation.status}</span>
                {Object.keys(detail.generation.parameters_snapshot).length > 0 && (
                  <code>{JSON.stringify(detail.generation.parameters_snapshot)}</code>
                )}
              </div>
            )}
          </section>
        )}

        {!editing && !promptText && !loading && (
          <section className="drawer-section missing-context">
            <span className="eyebrow">待整理</span>
            <h3>这份素材还没有 Prompt 资料</h3>
            <p>现在可以关联已有模型变体，并补充实际 Prompt、参数、评分与复盘。</p>
            <button className="button" type="button" onClick={startEditing}><Icon name="edit" size={15} /> 补充资料</button>
          </section>
        )}

        {!editing && detail?.evaluation?.lesson && (
          <section className="drawer-section review-section">
            <span className="eyebrow">复盘</span>
            <p>{detail.evaluation.lesson}</p>
          </section>
        )}

        {detail?.mime_type && (
          <section className="drawer-section file-section">
            <span className="eyebrow">文件信息</span>
            <div className="file-facts">
              <span>{detail.mime_type}</span>
              {detail.byte_size && <span>{(detail.byte_size / 1024 / 1024).toFixed(2)} MB</span>}
              {detail.related_asset_count > 0 && <span>{detail.related_asset_count} 份关联素材</span>}
            </div>
          </section>
        )}

        <section className="drawer-section asset-tag-section">
          <div className="section-heading">
            <div><span className="eyebrow">标签</span><h3>用于整理与筛选</h3></div>
            {!editingTags && <button className="copy-button" type="button" onClick={startTagEditing}><Icon name="edit" size={13} /> 编辑</button>}
          </div>
          {editingTags ? (
            <form onSubmit={(event) => void saveTags(event)}>
              <div className="tag-editor-list">
                {tagDraft.map((tag) => (
                  <button type="button" key={tag} onClick={() => setTagDraft((values) => values.filter((value) => value !== tag))}>
                    #{tag}<Icon name="close" size={11} />
                  </button>
                ))}
              </div>
              <div className="tag-editor-input">
                <Icon name="tag" size={14} />
                <input
                  value={tagInput}
                  maxLength={128}
                  onChange={(event) => setTagInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === "," || event.key === "，") {
                      event.preventDefault();
                      addTags(tagInput);
                    }
                  }}
                  placeholder="输入标签，按回车添加"
                  autoFocus
                />
                <button type="button" disabled={!tagInput.trim()} onClick={() => addTags(tagInput)}>添加</button>
              </div>
              {availableTags.filter((tag) => (
                !tagDraft.some((value) => value.toLocaleLowerCase() === tag.toLocaleLowerCase())
                && tag.toLocaleLowerCase().includes(tagInput.trim().replace(/^[#＃]+/, "").toLocaleLowerCase())
              )).slice(0, 6).length > 0 && (
                <div className="tag-suggestions">
                  <span>已有标签</span>
                  {availableTags.filter((tag) => (
                    !tagDraft.some((value) => value.toLocaleLowerCase() === tag.toLocaleLowerCase())
                    && tag.toLocaleLowerCase().includes(tagInput.trim().replace(/^[#＃]+/, "").toLocaleLowerCase())
                  )).slice(0, 6).map((tag) => (
                    <button type="button" key={tag} onClick={() => addTags(tag)}>#{tag}</button>
                  ))}
                </div>
              )}
              <small>{tagDraft.length} / 20 · 支持使用逗号一次添加多个</small>
              {isDemo && <p className="context-demo-note">浏览器示例可体验标签编辑，实际保存请使用 Tauri 桌面端。</p>}
              {tagError && <p className="context-edit-error">{tagError}</p>}
              <footer>
                <button className="button" type="button" onClick={() => setEditingTags(false)}>取消</button>
                <button className="button primary" disabled={writing || isDemo}>
                  {writing ? "保存中…" : isDemo ? "桌面端可保存" : "保存标签"}
                </button>
              </footer>
            </form>
          ) : displayTags.length > 0 ? (
            <div className="tag-row">{displayTags.map((tag) => <span key={tag}>#{tag}</span>)}</div>
          ) : (
            <button className="empty-tag-action" type="button" onClick={startTagEditing}><Icon name="plus" size={13} /> 添加第一个标签</button>
          )}
        </section>

        {!editing && (
          <footer>
            <button className="button" disabled={!originalUrl} onClick={() => setPreviewOpen(true)}><Icon name="expand" size={16} /> 查看原件</button>
            <button className="button primary" disabled={loading} onClick={startEditing}><Icon name="edit" size={16} /> 编辑资料</button>
          </footer>
        )}
      </aside>
      {previewOpen && originalUrl && (
        <OriginalPreview asset={asset} url={originalUrl} dimensions={dimensions} onClose={() => setPreviewOpen(false)} />
      )}
    </div>
  );
}

export function AssetLibrary({
  assets,
  total,
  models,
  query,
  loading,
  refreshing,
  displayMode,
  isDemo,
  prompts,
  availableTags,
  writing,
  onQueryChange,
  onDisplayModeChange,
  onImport,
  onLoadDetail,
  onUpdateAssetContext,
  onUpdateAssetTags,
}: AssetLibraryProps) {
  const [selectedAsset, setSelectedAsset] = useState<AssetSummary | null>(null);
  const hasFilters = Boolean(
    query.query ||
    (query.media_type && query.media_type !== "all") ||
    query.model ||
    query.min_rating ||
    query.featured_only ||
    query.tags?.length,
  );

  const resetQuery = () => onQueryChange({ media_type: "all", sort: query.sort ?? "newest" });
  const modelOptions: Array<SelectMenuOption<string>> = [
    { value: "", label: "所有模型", description: "跨模型与本地导入素材" },
    ...models.map((model) => ({ value: model, label: model, description: "只查看该模型的生成结果" })),
  ];

  return (
    <div className="view-shell library-view">
      <section className="view-intro">
        <div>
          <div className="title-with-badge">
            <h1>素材库</h1>
            {isDemo && <span className="demo-badge">示例数据</span>}
          </div>
          <p>让每一份结果都能回到它的 Prompt、模型与创作判断。</p>
        </div>
        <button className="button primary desktop-import" onClick={onImport}>
          <Icon name="upload" size={16} /> 导入素材
        </button>
      </section>

      <div className="library-toolbar">
        <label className="search-box">
          <Icon name="search" size={17} />
          <input
            type="search"
            value={query.query ?? ""}
            onChange={(event) => onQueryChange({ ...query, query: event.target.value })}
            placeholder="搜索标题、Prompt、模型或标签…"
            aria-label="搜索素材库"
          />
          {!query.query && <kbd>⌘ K</kbd>}
          {query.query && (
            <button onClick={() => onQueryChange({ ...query, query: "" })} aria-label="清除搜索" type="button">
              <Icon name="close" size={14} />
            </button>
          )}
        </label>

        <div className="media-tabs" role="tablist" aria-label="素材类型">
          {mediaFilters.map((filter) => (
            <button
              key={filter.value}
              className={(query.media_type ?? "all") === filter.value ? "is-active" : ""}
              onClick={() => onQueryChange({ ...query, media_type: filter.value })}
              role="tab"
              aria-selected={(query.media_type ?? "all") === filter.value}
            >
              {filter.label}
            </button>
          ))}
        </div>
      </div>

      <div className="filter-row">
        <SelectMenu
          value={query.model ?? ""}
          options={modelOptions}
          label="筛选模型"
          icon="sliders"
          onChange={(model) => onQueryChange({ ...query, model: model || undefined })}
        />
        <TagFilterMenu
          tags={availableTags}
          selected={query.tags ?? []}
          onChange={(tags) => onQueryChange({ ...query, tags: tags.length ? tags : undefined })}
        />
        <button
          className={`filter-chip ${query.min_rating === 4 ? "is-active" : ""}`}
          onClick={() => onQueryChange({ ...query, min_rating: query.min_rating === 4 ? undefined : 4 })}
        >
          <Icon name="star" size={14} /> 4 分以上
        </button>
        <button
          className={`filter-chip ${query.featured_only ? "is-active" : ""}`}
          onClick={() => onQueryChange({ ...query, featured_only: !query.featured_only })}
        >
          精选
        </button>
        {hasFilters && <button className="reset-filter" onClick={resetQuery}>重置</button>}
        <span className={`result-count ${refreshing ? "is-refreshing" : ""}`}>
          {loading ? "正在载入…" : refreshing ? "正在更新…" : `${total} 份素材`}
        </span>
        <SelectMenu
          value={query.sort ?? "newest"}
          options={sortOptions}
          label="排序方式"
          icon="sort"
          align="end"
          className="sort-control"
          onChange={(sort) => onQueryChange({ ...query, sort })}
        />
        <div className="display-switcher" role="group" aria-label="素材展示方式">
          <button
            type="button"
            className={displayMode === "card" ? "is-active" : ""}
            onClick={() => onDisplayModeChange("card")}
            aria-label="卡片视图"
            aria-pressed={displayMode === "card"}
            title="卡片视图"
          >
            <Icon name="grid" size={14} />
          </button>
          <button
            type="button"
            className={displayMode === "list" ? "is-active" : ""}
            onClick={() => onDisplayModeChange("list")}
            aria-label="列表视图"
            aria-pressed={displayMode === "list"}
            title="列表视图"
          >
            <Icon name="list" size={15} />
          </button>
        </div>
      </div>

      {(query.tags?.length ?? 0) > 0 && (
        <div className="selected-tag-filters" aria-label="已选标签">
          <span>同时包含</span>
          {query.tags?.map((tag) => (
            <button
              type="button"
              key={tag}
              onClick={() => {
                const tags = query.tags?.filter((value) => value !== tag) ?? [];
                onQueryChange({ ...query, tags: tags.length ? tags : undefined });
              }}
            >
              #{tag}<Icon name="close" size={10} />
            </button>
          ))}
        </div>
      )}

      {loading ? (
        displayMode === "card" ? (
          <div className="asset-grid">{Array.from({ length: 8 }, (_, index) => <AssetSkeleton key={index} />)}</div>
        ) : (
          <div className="asset-list">{Array.from({ length: 6 }, (_, index) => <AssetListSkeleton key={index} />)}</div>
        )
      ) : assets.length > 0 ? (
        displayMode === "card" ? (
          <div className="asset-grid">
            {assets.map((asset) => <AssetCard key={asset.id} asset={asset} onOpen={() => setSelectedAsset(asset)} />)}
          </div>
        ) : (
          <div className="asset-list">
            {assets.map((asset) => <AssetListRow key={asset.id} asset={asset} onOpen={() => setSelectedAsset(asset)} />)}
          </div>
        )
      ) : (
        <EmptyLibrary filtered={hasFilters} onReset={resetQuery} onImport={onImport} />
      )}

      {selectedAsset && (
        <AssetDrawer
          asset={selectedAsset}
          prompts={prompts}
          availableTags={availableTags}
          writing={writing}
          isDemo={isDemo}
          onClose={() => setSelectedAsset(null)}
          onLoadDetail={onLoadDetail}
          onUpdateAssetContext={onUpdateAssetContext}
          onUpdateAssetTags={onUpdateAssetTags}
        />
      )}
    </div>
  );
}
