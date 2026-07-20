import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
  type FormEvent,
  type ReactNode,
} from "react";
import type {
  CreatePromptInput,
  ImportAssetOptions,
  ProjectSummary,
  PromptSummary,
  PromptVariantInput,
} from "../lib/library-client";
import { Icon } from "./Icon";
import { SelectMenu, type SelectMenuOption } from "./SelectMenu";

const IMPORT_MIME_BY_EXTENSION: Record<string, string> = {
  avif: "image/avif",
  jpeg: "image/jpeg",
  jpg: "image/jpeg",
  mov: "video/quicktime",
  mp4: "video/mp4",
  png: "image/png",
  webm: "video/webm",
  webp: "image/webp",
};

const IMPORT_EXTENSION_BY_MIME: Record<string, string> = {
  "image/avif": "avif",
  "image/jpeg": "jpg",
  "image/png": "png",
  "image/webp": "webp",
  "video/mp4": "mp4",
  "video/quicktime": "mov",
  "video/webm": "webm",
};

const IMPORT_MIME_TYPES = new Set(Object.keys(IMPORT_EXTENSION_BY_MIME));

function importMimeType(file: File): string | null {
  const declared = file.type.toLowerCase() === "image/jpg" ? "image/jpeg" : file.type.toLowerCase();
  if (IMPORT_MIME_TYPES.has(declared)) return declared;
  const extension = file.name.split(".").pop()?.toLowerCase();
  return extension ? IMPORT_MIME_BY_EXTENSION[extension] ?? null : null;
}

function normalizeImportFile(file: File): File | null {
  const mimeType = importMimeType(file);
  if (!mimeType) return null;
  if (file.type === mimeType) return file;
  return new File([file], file.name, { type: mimeType, lastModified: file.lastModified });
}

function importFileKey(file: File): string {
  return [file.name, file.size, file.type, file.lastModified].join(":");
}

function clipboardFilename(mimeType: string, index: number): string {
  const now = new Date();
  const timestamp = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
    "-",
    String(now.getHours()).padStart(2, "0"),
    String(now.getMinutes()).padStart(2, "0"),
    String(now.getSeconds()).padStart(2, "0"),
  ].join("");
  const suffix = index > 0 ? `-${index + 1}` : "";
  return `clipboard-${timestamp}${suffix}.${IMPORT_EXTENSION_BY_MIME[mimeType]}`;
}

interface ModalShellProps {
  open: boolean;
  eyebrow: string;
  title: string;
  label: string;
  onClose: () => void;
  children: ReactNode;
  className?: string;
}

function ModalShell({ open, eyebrow, title, label, onClose, children, className = "" }: ModalShellProps) {
  if (!open) return null;
  return (
    <div className="modal-layer" role="dialog" aria-modal="true" aria-label={label}>
      <button className="modal-scrim" onClick={onClose} aria-label={`关闭${label}`} />
      <section className={`entity-dialog ${className}`.trim()}>
        <header>
          <div><span className="eyebrow">{eyebrow}</span><h2>{title}</h2></div>
          <button className="icon-button" onClick={onClose} aria-label="关闭"><Icon name="close" /></button>
        </header>
        {children}
      </section>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="form-field">
      <span><strong>{label}</strong>{hint && <small>{hint}</small>}</span>
      {children}
    </label>
  );
}

export function CreateProjectDialog({
  open,
  busy,
  onClose,
  onSubmit,
}: {
  open: boolean;
  busy: boolean;
  onClose: () => void;
  onSubmit: (input: { name: string; description?: string }) => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setDescription("");
    }
  }, [open]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    await onSubmit({ name: name.trim(), description: description.trim() || undefined });
  };

  return (
    <ModalShell open={open} eyebrow="组织边界" title="新建项目" label="新建项目" onClose={onClose}>
      <form className="entity-form" onSubmit={(event) => void submit(event)}>
        <Field label="项目名称" hint="品牌、活动或长期主题">
          <input autoFocus value={name} onChange={(event) => setName(event.target.value)} placeholder="例如：城市情绪研究" />
        </Field>
        <Field label="项目说明" hint="可稍后补充">
          <textarea value={description} onChange={(event) => setDescription(event.target.value)} placeholder="这组创作想解决什么问题？" rows={4} />
        </Field>
        <footer>
          <button type="button" className="button" onClick={onClose}>取消</button>
          <button className="button primary" disabled={busy || !name.trim()}>{busy ? "正在创建…" : "创建项目"}</button>
        </footer>
      </form>
    </ModalShell>
  );
}

export function CreatePromptDialog({
  open,
  projects,
  busy,
  onClose,
  onNeedProject,
  onSubmit,
}: {
  open: boolean;
  projects: ProjectSummary[];
  busy: boolean;
  onClose: () => void;
  onNeedProject: () => void;
  onSubmit: (input: CreatePromptInput) => Promise<void>;
}) {
  const [projectId, setProjectId] = useState("");
  const [title, setTitle] = useState("");
  const [intent, setIntent] = useState("");
  const [masterPrompt, setMasterPrompt] = useState("");
  const [negative, setNegative] = useState("");
  const [provider, setProvider] = useState("OpenAI");
  const [model, setModel] = useState("GPT Image 2");
  const [variantPrompt, setVariantPrompt] = useState("");

  useEffect(() => {
    if (!open) return;
    setProjectId(projects[0]?.id ?? "");
    setTitle("");
    setIntent("");
    setMasterPrompt("");
    setNegative("");
    setProvider("OpenAI");
    setModel("GPT Image 2");
    setVariantPrompt("");
  }, [open, projects]);

  const projectOptions: Array<SelectMenuOption<string>> = projects.map((project) => ({
    value: project.id,
    label: project.name,
    description: project.description ?? "本地项目",
  }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!projectId || !title.trim() || !intent.trim() || !masterPrompt.trim()) return;
    await onSubmit({
      project_id: projectId,
      title: title.trim(),
      creative_intent: intent.trim(),
      base_prompt: masterPrompt.trim(),
      negative_constraints: negative.trim() || undefined,
      initial_variant: variantPrompt.trim() && provider.trim() && model.trim() ? {
        provider: provider.trim(),
        model: model.trim(),
        prompt_text: variantPrompt.trim(),
      } : undefined,
    });
  };

  return (
    <ModalShell open={open} eyebrow="创作母版" title="新建 Prompt" label="新建 Prompt" onClose={onClose} className="wide-dialog">
      {projects.length === 0 ? (
        <div className="dialog-prerequisite">
          <span><Icon name="folder" size={22} /></span>
          <h3>先建立一个项目</h3>
          <p>Prompt 需要归入一个项目，之后素材和生成记录才能形成清晰链路。</p>
          <button className="button primary" onClick={onNeedProject}><Icon name="plus" size={15} /> 新建项目</button>
        </div>
      ) : (
        <form className="entity-form" onSubmit={(event) => void submit(event)}>
          <Field label="所属项目">
            <SelectMenu value={projectId} options={projectOptions} label="选择项目" icon="folder" onChange={setProjectId} className="form-select" />
          </Field>
          <div className="form-grid">
            <Field label="Prompt 标题"><input value={title} onChange={(event) => setTitle(event.target.value)} placeholder="清晰描述这份创作方法" /></Field>
            <Field label="创作意图"><input value={intent} onChange={(event) => setIntent(event.target.value)} placeholder="希望画面达成什么效果？" /></Field>
          </div>
          <Field label="母版 Prompt" hint="不依赖具体平台的共同意图">
            <textarea value={masterPrompt} onChange={(event) => setMasterPrompt(event.target.value)} rows={4} placeholder="描述主体、环境、构图、光线和情绪…" />
          </Field>
          <Field label="负向约束" hint="可选"><textarea value={negative} onChange={(event) => setNegative(event.target.value)} rows={2} placeholder="不希望出现的元素或风格…" /></Field>
          <div className="variant-draft-box">
            <div><span className="eyebrow">首个模型变体 · 可选</span><p>现在录入实际使用版本，导入素材时就能直接关联。</p></div>
            <div className="form-grid">
              <Field label="平台"><input value={provider} onChange={(event) => setProvider(event.target.value)} /></Field>
              <Field label="模型"><input value={model} onChange={(event) => setModel(event.target.value)} /></Field>
            </div>
            <Field label="实际 Prompt"><textarea value={variantPrompt} onChange={(event) => setVariantPrompt(event.target.value)} rows={3} placeholder="留空则只创建母版" /></Field>
          </div>
          <footer>
            <button type="button" className="button" onClick={onClose}>取消</button>
            <button className="button primary" disabled={busy || !projectId || !title.trim() || !intent.trim() || !masterPrompt.trim()}>
              {busy ? "正在保存…" : "创建 Prompt"}
            </button>
          </footer>
        </form>
      )}
    </ModalShell>
  );
}

export function CreateVariantDialog({
  prompt,
  open,
  busy,
  onClose,
  onSubmit,
}: {
  prompt: PromptSummary | null;
  open: boolean;
  busy: boolean;
  onClose: () => void;
  onSubmit: (promptId: string, input: PromptVariantInput) => Promise<void>;
}) {
  const [provider, setProvider] = useState("");
  const [model, setModel] = useState("");
  const [promptText, setPromptText] = useState("");
  const [parameters, setParameters] = useState("");
  const [notes, setNotes] = useState("");

  useEffect(() => {
    if (open) {
      setProvider("");
      setModel("");
      setPromptText(prompt?.master_prompt ?? "");
      setParameters("");
      setNotes("");
    }
  }, [open, prompt]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!prompt || !provider.trim() || !model.trim() || !promptText.trim()) return;
    let parsedParameters: Record<string, unknown> = {};
    if (parameters.trim()) {
      parsedParameters = JSON.parse(parameters) as Record<string, unknown>;
    }
    await onSubmit(prompt.id, {
      provider: provider.trim(),
      model: model.trim(),
      prompt_text: promptText.trim(),
      parameters: parsedParameters,
      adaptation_notes: notes.trim() || undefined,
    });
  };

  return (
    <ModalShell open={open} eyebrow={prompt?.title ?? "模型变体"} title="添加模型变体" label="添加模型变体" onClose={onClose}>
      <form className="entity-form" onSubmit={(event) => void submit(event)}>
        <div className="form-grid">
          <Field label="平台"><input autoFocus value={provider} onChange={(event) => setProvider(event.target.value)} placeholder="OpenAI / ByteDance" /></Field>
          <Field label="模型"><input value={model} onChange={(event) => setModel(event.target.value)} placeholder="GPT Image 2 / Seedream" /></Field>
        </div>
        <Field label="实际 Prompt"><textarea value={promptText} onChange={(event) => setPromptText(event.target.value)} rows={5} /></Field>
        <Field label="参数 JSON" hint="可选"><textarea value={parameters} onChange={(event) => setParameters(event.target.value)} rows={2} placeholder='{"aspect_ratio":"4:5"}' /></Field>
        <Field label="适配备注" hint="记录该模型的表达偏好"><textarea value={notes} onChange={(event) => setNotes(event.target.value)} rows={3} /></Field>
        <footer>
          <button type="button" className="button" onClick={onClose}>取消</button>
          <button className="button primary" disabled={busy || !provider.trim() || !model.trim() || !promptText.trim()}>
            {busy ? "正在保存…" : "添加变体"}
          </button>
        </footer>
      </form>
    </ModalShell>
  );
}

export function ImportDialog({
  open,
  prompts,
  busy,
  storagePath,
  storageRestartPending = false,
  onClose,
  onSubmit,
}: {
  open: boolean;
  prompts: PromptSummary[];
  busy: boolean;
  storagePath?: string;
  storageRestartPending?: boolean;
  onClose: () => void;
  onSubmit: (files: File[], options: ImportAssetOptions) => Promise<void>;
}) {
  const [files, setFiles] = useState<File[]>([]);
  const [variantId, setVariantId] = useState("");
  const [rating, setRating] = useState<number | undefined>();
  const [featured, setFeatured] = useState(false);
  const [lesson, setLesson] = useState("");
  const [dragging, setDragging] = useState(false);
  const [fileFeedback, setFileFeedback] = useState<{ tone: "info" | "error"; text: string } | null>(null);
  const dragDepth = useRef(0);

  useEffect(() => {
    if (open) {
      setFiles([]);
      setVariantId("");
      setRating(undefined);
      setFeatured(false);
      setLesson("");
      setDragging(false);
      setFileFeedback(null);
      dragDepth.current = 0;
    }
  }, [open]);

  const addFiles = useCallback((incoming: File[], source: "picker" | "drop" | "paste") => {
    const normalized: File[] = [];
    let rejected = 0;
    for (const file of incoming) {
      const accepted = normalizeImportFile(file);
      if (accepted) normalized.push(accepted);
      else rejected += 1;
    }

    const known = new Set(files.map(importFileKey));
    const additions = normalized.filter((file) => {
      const key = importFileKey(file);
      if (known.has(key)) return false;
      known.add(key);
      return true;
    });
    const duplicates = normalized.length - additions.length;

    if (additions.length > 0) setFiles([...files, ...additions]);

    const sourceLabel = source === "paste" ? "粘贴" : source === "drop" ? "拖入" : "选择";
    const messages = [
      additions.length > 0 ? `已${sourceLabel} ${additions.length} 份素材` : "",
      duplicates > 0 ? `忽略 ${duplicates} 份重复文件` : "",
      rejected > 0 ? `${rejected} 份格式暂不支持` : "",
    ].filter(Boolean);
    setFileFeedback(messages.length > 0
      ? { tone: rejected > 0 && additions.length === 0 ? "error" : "info", text: messages.join("，") }
      : null);
  }, [files]);

  useEffect(() => {
    if (!open) return;
    const handlePaste = (event: ClipboardEvent) => {
      const pasted = Array.from(event.clipboardData?.files ?? []);
      if (pasted.length === 0) return;
      event.preventDefault();
      const named = pasted.map((file, index) => {
        const mimeType = importMimeType(file);
        if (!mimeType) return file;
        return new File([file], clipboardFilename(mimeType, index), {
          type: mimeType,
          lastModified: file.lastModified,
        });
      });
      addFiles(named, "paste");
    };
    window.addEventListener("paste", handlePaste);
    return () => window.removeEventListener("paste", handlePaste);
  }, [addFiles, open]);

  const hasDraggedFiles = (event: ReactDragEvent<HTMLElement>) => (
    Array.from(event.dataTransfer.types).includes("Files")
  );

  const handleDragEnter = (event: ReactDragEvent<HTMLLabelElement>) => {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    dragDepth.current += 1;
    setDragging(true);
  };

  const handleDragOver = (event: ReactDragEvent<HTMLLabelElement>) => {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  };

  const handleDragLeave = (event: ReactDragEvent<HTMLLabelElement>) => {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDragging(false);
  };

  const handleDrop = (event: ReactDragEvent<HTMLLabelElement>) => {
    event.preventDefault();
    dragDepth.current = 0;
    setDragging(false);
    addFiles(Array.from(event.dataTransfer.files), "drop");
  };

  const variants = useMemo(
    () => prompts.flatMap((prompt) => prompt.variants.map((variant) => ({ prompt, variant }))),
    [prompts],
  );
  const selected = variants.find((item) => item.variant.id === variantId);
  const variantOptions: Array<SelectMenuOption<string>> = [
    { value: "", label: "仅导入，稍后整理", description: "作为未归档素材保存" },
    ...variants.map(({ prompt, variant }) => ({
      value: variant.id,
      label: `${variant.model} · ${prompt.title}`,
      description: variant.platform,
    })),
  ];

  return (
    <ModalShell open={open} eyebrow="生成结果" title="导入素材" label="导入素材" onClose={onClose} className="import-dialog wide-dialog">
      <form className="entity-form" onSubmit={(event) => {
        event.preventDefault();
        void onSubmit(files, {
          variant_id: variantId || undefined,
          prompt_text: selected?.variant.prompt,
          rating,
          featured,
          lesson: lesson.trim() || undefined,
        });
      }}>
        <label
          className={`drop-zone compact-drop ${files.length ? "has-files" : ""} ${dragging ? "is-dragging" : ""}`.trim()}
          onDragEnter={handleDragEnter}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
        >
          <input
            id="asset-import-files"
            type="file"
            multiple
            accept=".png,.jpg,.jpeg,.webp,.avif,.mp4,.mov,.webm,image/png,image/jpeg,image/webp,image/avif,video/mp4,video/quicktime,video/webm"
            onChange={(event) => {
              addFiles(Array.from(event.target.files ?? []), "picker");
              event.currentTarget.value = "";
            }}
          />
          <span className="drop-icon"><Icon name={files.length && !dragging ? "check" : "upload"} size={22} /></span>
          <strong>{dragging ? "松开即可加入素材" : files.length ? `已选择 ${files.length} 份素材` : "点击选择或拖入图片、视频"}</strong>
          <span>
            {dragging
              ? "支持 PNG、JPEG、WebP、AVIF、MP4、MOV 与 WebM"
              : files.length
                ? `${files.map((file) => file.name).slice(0, 3).join(" · ")}${files.length > 3 ? ` · 另 ${files.length - 3} 份` : ""}`
                : "原件保持不变，图片会自动生成 WebP 缩略图"}
          </span>
          <small>也可按 ⌘/Ctrl + V 粘贴剪贴板图片</small>
        </label>
        {fileFeedback && <p className={`drop-feedback ${fileFeedback.tone}`} role="status" aria-live="polite">{fileFeedback.text}</p>}
        <div className="association-panel">
          <div><span className="eyebrow">关联方式</span><p>推荐在导入时绑定模型变体，之后可以从素材反查实际 Prompt。</p></div>
          <Field label="生成自">
            <SelectMenu value={variantId} options={variantOptions} label="选择模型变体" icon="sparkles" onChange={setVariantId} className="form-select" />
          </Field>
          {variantId && (
            <>
              <div className="score-field">
                <span><strong>初始评分</strong><small>可选</small></span>
                <div>{[1, 2, 3, 4, 5].map((score) => (
                  <button type="button" key={score} className={rating === score ? "is-active" : ""} onClick={() => setRating(rating === score ? undefined : score)}>
                    <Icon name="star" size={13} /> {score}
                  </button>
                ))}</div>
              </div>
              <label className="check-field">
                <input type="checkbox" checked={featured} onChange={(event) => setFeatured(event.target.checked)} />
                <span><strong>标记为精选</strong><small>在素材库中优先筛选</small></span>
              </label>
              <Field label="一句复盘" hint="可选"><input value={lesson} onChange={(event) => setLesson(event.target.value)} placeholder="为什么这次结果值得保留？" /></Field>
            </>
          )}
        </div>
        <div className="import-note import-storage-note">
          <Icon name="database" size={17} />
          <span>
            <strong>媒体保存位置</strong>
            <code title={storagePath}>{storagePath ?? "正在读取存储配置…"}</code>
            <small>
              {storageRestartPending
                ? "当前导入仍写入此目录；新目录将在重启后生效。"
                : "原件和缩略图写入此目录，元数据与生成链路写入数据库。"}
            </small>
          </span>
        </div>
        <footer>
          <button type="button" className="button" onClick={onClose}>取消</button>
          <button className="button primary" disabled={busy || files.length === 0}>{busy ? "正在导入…" : `导入 ${files.length || ""} 份素材`}</button>
        </footer>
      </form>
    </ModalShell>
  );
}
