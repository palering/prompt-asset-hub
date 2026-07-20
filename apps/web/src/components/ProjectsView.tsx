import type { ProjectSummary, PromptSummary } from "../lib/library-client";
import { Icon } from "./Icon";

function formatDate(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", { month: "short", day: "numeric" }).format(new Date(value));
}

export function ProjectsView({
  projects,
  prompts,
  loading,
  isDemo,
  onCreate,
  onOpenPrompts,
}: {
  projects: ProjectSummary[];
  prompts: PromptSummary[];
  loading: boolean;
  isDemo: boolean;
  onCreate: () => void;
  onOpenPrompts: () => void;
}) {
  return (
    <div className="view-shell projects-view">
      <section className="view-intro">
        <div>
          <div className="title-with-badge">
            <h1>项目</h1>
            {isDemo && <span className="demo-badge">示例数据</span>}
          </div>
          <p>按品牌、活动或创作主题聚合 Prompt 与结果素材。</p>
        </div>
        <button className="button primary" onClick={onCreate}><Icon name="plus" size={16} /> 新建项目</button>
      </section>

      {loading ? (
        <div className="project-grid">{Array.from({ length: 4 }, (_, index) => <div className="project-card project-skeleton" key={index} />)}</div>
      ) : projects.length === 0 ? (
        <div className="secondary-placeholder project-empty">
          <div className="placeholder-visual folders"><span><Icon name="folder" size={28} /></span><i /><i /><i /></div>
          <span className="eyebrow">第一层组织</span>
          <h2>先建立一个项目</h2>
          <p>它不会增加权限复杂度，只用于把相关 Prompt、模型变体和素材归在一起。</p>
          <button className="button primary" onClick={onCreate}><Icon name="plus" size={15} /> 新建项目</button>
        </div>
      ) : (
        <div className="project-grid">
          {projects.map((project) => {
            const projectPrompts = prompts.filter((prompt) => prompt.project_id === project.id);
            const variants = projectPrompts.reduce((total, prompt) => total + prompt.variants.length, 0);
            return (
              <article className="project-card" key={project.id}>
                <header>
                  <span className="project-icon"><Icon name="folder" size={19} /></span>
                  <time>{formatDate(project.updated_at)} 更新</time>
                </header>
                <h2>{project.name}</h2>
                <p>{project.description ?? "尚未添加项目说明。"}</p>
                <div className="project-stats">
                  <span><strong>{projectPrompts.length}</strong> Prompt</span>
                  <span><strong>{variants}</strong> 模型变体</span>
                </div>
                <footer>
                  <div className="project-prompt-dots">
                    {projectPrompts.slice(0, 4).map((prompt) => <span key={prompt.id} title={prompt.title}>{prompt.title.slice(0, 1)}</span>)}
                  </div>
                  <button className="text-button" onClick={onOpenPrompts}>打开工作台 <Icon name="arrow" size={13} /></button>
                </footer>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}
