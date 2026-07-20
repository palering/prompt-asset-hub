import { useEffect, useMemo, useState } from "react";
import type { PromptStatus, PromptSummary, PromptVariant } from "../lib/library-client";
import { Icon } from "./Icon";

interface PromptWorkbenchProps {
  prompts: PromptSummary[];
  loading: boolean;
  isDemo: boolean;
  onCreate: () => void;
  onAddVariant: (prompt: PromptSummary) => void;
}

const statusLabels: Record<PromptStatus, string> = {
  draft: "草稿",
  verified: "已验证",
  archived: "已归档",
};

function shortDate(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(new Date(value));
}

function VariantCard({
  variant,
  selected,
  onSelect,
}: {
  variant: PromptVariant;
  selected: boolean;
  onSelect: () => void;
}) {
  const initials = variant.model
    .split(/\s+/)
    .map((word) => word[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();

  return (
    <button className={`variant-card ${selected ? "is-selected" : ""}`} onClick={onSelect}>
      <span className="model-avatar">{initials}</span>
      <span className="variant-main">
        <span className="variant-name-row">
          <strong>{variant.model}</strong>
          <small className={`status-${variant.status}`}>{statusLabels[variant.status]}</small>
        </span>
        <span>{variant.parameter_summary ?? variant.platform}</span>
      </span>
      <span className="variant-score">
        {variant.rating != null ? <><Icon name="star" size={12} /> {variant.rating.toFixed(1)}</> : "待验证"}
      </span>
      <Icon name="chevron" size={14} />
    </button>
  );
}

function EmptyPrompts({ onCreate }: { onCreate: () => void }) {
  return (
    <div className="prompt-empty">
      <div className="empty-prompt-mark"><Icon name="sparkles" size={28} /></div>
      <h2>建立第一份 Prompt 母版</h2>
      <p>先记录不依赖平台的创作意图，之后再为 GPT Image、Seedream 等模型维护各自的表达。</p>
      <button className="button primary" onClick={onCreate}><Icon name="plus" size={16} /> 新建 Prompt</button>
    </div>
  );
}

function WorkbenchSkeleton() {
  return (
    <div className="workbench-skeleton">
      <aside><span /><span /><span /></aside>
      <main><span /><span /><span className="large" /></main>
    </div>
  );
}

export function PromptWorkbench({ prompts, loading, isDemo, onCreate, onAddVariant }: PromptWorkbenchProps) {
  const [selectedPromptId, setSelectedPromptId] = useState<string | null>(null);
  const [selectedVariantId, setSelectedVariantId] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!selectedPromptId && prompts[0]) setSelectedPromptId(prompts[0].id);
  }, [prompts, selectedPromptId]);

  const selectedPrompt = useMemo(
    () => prompts.find((prompt) => prompt.id === selectedPromptId) ?? prompts[0] ?? null,
    [prompts, selectedPromptId],
  );

  useEffect(() => {
    if (!selectedPrompt) return;
    const selectionIsValid = selectedPrompt.variants.some((variant) => variant.id === selectedVariantId);
    if (!selectionIsValid) setSelectedVariantId(selectedPrompt.variants[0]?.id ?? null);
  }, [selectedPrompt, selectedVariantId]);

  const selectedVariant = selectedPrompt?.variants.find((variant) => variant.id === selectedVariantId)
    ?? selectedPrompt?.variants[0]
    ?? null;

  const copyPrompt = (text: string) => {
    void navigator.clipboard?.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };

  return (
    <div className="view-shell prompt-view">
      <section className="view-intro">
        <div>
          <div className="title-with-badge">
            <h1>Prompt 工作台</h1>
            {isDemo && <span className="demo-badge">跨模型示例</span>}
          </div>
          <p>保留共同意图，同时沉淀每个平台真正有效的表达方式。</p>
        </div>
        <button className="button primary" onClick={onCreate}><Icon name="plus" size={16} /> 新建 Prompt</button>
      </section>

      {loading ? <WorkbenchSkeleton /> : !selectedPrompt ? <EmptyPrompts onCreate={onCreate} /> : (
        <div className="workbench">
          <aside className="prompt-list-panel">
            <header>
              <span>{prompts.length} 份 Prompt</span>
              <button className="quiet-icon" aria-label="筛选 Prompt"><Icon name="sliders" size={16} /></button>
            </header>
            <label className="prompt-search">
              <Icon name="search" size={15} />
              <input placeholder="查找 Prompt…" aria-label="查找 Prompt" />
            </label>
            <div className="prompt-list">
              {prompts.map((prompt) => (
                <button
                  key={prompt.id}
                  className={selectedPrompt.id === prompt.id ? "is-active" : ""}
                  onClick={() => setSelectedPromptId(prompt.id)}
                >
                  <span className="prompt-list-icon"><Icon name="sparkles" size={15} /></span>
                  <span>
                    <strong>{prompt.title}</strong>
                    <small>{prompt.variants.length} 个模型变体 · {shortDate(prompt.updated_at)} 更新</small>
                  </span>
                  <span className={`status-dot-label status-${prompt.status}`}>{statusLabels[prompt.status]}</span>
                </button>
              ))}
            </div>
          </aside>

          <main className="prompt-editor-panel">
            <header className="prompt-editor-header">
              <div>
                <div className="prompt-status-line">
                  <span className={`status-tag status-${selectedPrompt.status}`}>{statusLabels[selectedPrompt.status]}</span>
                  <span>最近更新 {shortDate(selectedPrompt.updated_at)}</span>
                </div>
                <h2>{selectedPrompt.title}</h2>
                <p>{selectedPrompt.intent}</p>
              </div>
              <button className="icon-button" aria-label="更多 Prompt 操作"><Icon name="more" /></button>
            </header>

            <div className="tag-row prompt-tags">
              {selectedPrompt.tags.map((tag) => <span key={tag}>#{tag}</span>)}
            </div>

            <section className="master-prompt-card">
              <header>
                <div>
                  <span className="eyebrow">母版 Prompt</span>
                  <h3>平台无关的创作意图</h3>
                </div>
                <button className="copy-button" onClick={() => copyPrompt(selectedPrompt.master_prompt)}>
                  <Icon name={copied ? "check" : "copy"} size={14} /> {copied ? "已复制" : "复制"}
                </button>
              </header>
              <p>{selectedPrompt.master_prompt}</p>
            </section>

            <section className="variant-section">
              <div className="section-heading">
                <div><span className="eyebrow">模型变体</span><h3>已验证的表达与参数</h3></div>
                <button className="text-button" onClick={() => onAddVariant(selectedPrompt)}><Icon name="plus" size={14} /> 添加变体</button>
              </div>
              <div className="variant-layout">
                <div className="variant-list">
                  {selectedPrompt.variants.map((variant) => (
                    <VariantCard
                      key={variant.id}
                      variant={variant}
                      selected={variant.id === selectedVariant?.id}
                      onSelect={() => setSelectedVariantId(variant.id)}
                    />
                  ))}
                </div>

                {selectedVariant ? (
                  <div className="variant-detail">
                    <header>
                      <div>
                        <span className="eyebrow">{selectedVariant.platform}</span>
                        <h3>{selectedVariant.model}</h3>
                      </div>
                      <button className="copy-button" onClick={() => copyPrompt(selectedVariant.prompt)}><Icon name="copy" size={14} /> 复制组合</button>
                    </header>
                    <p className="variant-prompt-text">{selectedVariant.prompt}</p>
                    <div className="adaptation-note">
                      <Icon name="sparkles" size={15} />
                      <span><strong>适配备注</strong>{selectedVariant.adaptation_note ?? "尚未记录该模型的适配经验。"}</span>
                    </div>
                    <footer>
                      <span>{selectedVariant.result_count} 份生成结果</span>
                      <span>{selectedVariant.parameter_summary ?? "默认参数"}</span>
                    </footer>
                  </div>
                ) : (
                  <div className="variant-empty">添加第一个模型变体，开始比较效果。</div>
                )}
              </div>
            </section>

            <section className="results-section">
              <div className="section-heading">
                <div><span className="eyebrow">最近结果</span><h3>用输出验证 Prompt</h3></div>
                <button className="text-button">查看全部 <Icon name="arrow" size={14} /></button>
              </div>
              {isDemo && (selectedVariant?.result_count ?? 0) > 0 ? (
                <div className="result-strip">
                  {["nocturne", "afterrain", "limestone", "orbit"].map((palette, index) => (
                    <div key={palette} className={`mini-result palette-${palette}`}>
                      <span className="mini-result-index">0{index + 1}</span>
                      <span className="mini-result-rating"><Icon name="star" size={11} /> {(4.9 - index * 0.2).toFixed(1)}</span>
                    </div>
                  ))}
                  <button className="more-results"><strong>+{Math.max(0, (selectedVariant?.result_count ?? 0) - 4)}</strong><span>更多结果</span></button>
                </div>
              ) : (selectedVariant?.result_count ?? 0) > 0 ? (
                <div className="results-linked">
                  <span><Icon name="image" size={18} /></span>
                  <div><strong>{selectedVariant?.result_count} 份真实结果已关联</strong><small>可在素材库按模型筛选并打开生成链路。</small></div>
                </div>
              ) : (
                <div className="results-empty">导入生成结果并选择这个模型变体后，素材会出现在这里。</div>
              )}
            </section>
          </main>
        </div>
      )}
    </div>
  );
}
