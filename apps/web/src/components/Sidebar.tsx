import type { PointerEvent as ReactPointerEvent } from "react";
import { Icon, type IconName } from "./Icon";

export type ViewKey = "library" | "prompts" | "projects" | "activity";

interface NavItem {
  key: ViewKey;
  label: string;
  icon: IconName;
  badge?: number;
}

interface SidebarProps {
  current: ViewKey;
  assetCount: number;
  promptCount: number;
  open: boolean;
  collapsed: boolean;
  onNavigate: (view: ViewKey) => void;
  onClose: () => void;
  onToggleCollapsed: () => void;
  onResize: (width: number) => void;
  onPrimaryAction: () => void;
  onOpenSettings: () => void;
}

const navigation: NavItem[] = [
  { key: "library", label: "素材库", icon: "grid" },
  { key: "prompts", label: "Prompt 工作台", icon: "sparkles" },
  { key: "projects", label: "项目", icon: "folder" },
  { key: "activity", label: "Agent 记录", icon: "activity" },
];

export function Sidebar({
  current,
  assetCount,
  promptCount,
  open,
  collapsed,
  onNavigate,
  onClose,
  onToggleCollapsed,
  onResize,
  onPrimaryAction,
  onOpenSettings,
}: SidebarProps) {
  const getBadge = (key: ViewKey) => {
    if (key === "library") return assetCount;
    if (key === "prompts") return promptCount;
    return undefined;
  };
  const startResizing = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (collapsed || window.matchMedia("(max-width: 920px)").matches) return;
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = event.currentTarget.parentElement?.getBoundingClientRect().width ?? 238;
    document.body.classList.add("is-resizing-sidebar");

    const onPointerMove = (moveEvent: PointerEvent) => {
      onResize(Math.min(360, Math.max(210, startWidth + moveEvent.clientX - startX)));
    };
    const finishResizing = () => {
      document.body.classList.remove("is-resizing-sidebar");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", finishResizing);
      window.removeEventListener("pointercancel", finishResizing);
    };
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", finishResizing, { once: true });
    window.addEventListener("pointercancel", finishResizing, { once: true });
  };

  return (
    <>
      <button className={`sidebar-scrim ${open ? "is-visible" : ""}`} onClick={onClose} aria-label="关闭导航" />
      <aside className={`sidebar ${open ? "is-open" : ""} ${collapsed ? "is-collapsed" : ""}`}>
        <button
          className="sidebar-collapse"
          type="button"
          onClick={onToggleCollapsed}
          aria-label={collapsed ? "展开侧栏" : "收起侧栏"}
          title={collapsed ? "展开侧栏" : "收起侧栏"}
        >
          <Icon name="chevron" size={13} />
        </button>
        <button
          className="sidebar-resizer"
          type="button"
          onPointerDown={startResizing}
          onDoubleClick={() => onResize(238)}
          aria-label="拖拽调整侧栏宽度，双击恢复默认"
          title="拖拽调整宽度 · 双击恢复默认"
        />
        <div className="brand-row">
          <div className="brand-mark" aria-hidden="true">
            <span />
            <span />
            <span />
          </div>
          <div className="brand-copy">
            <strong>Prompt Asset</strong>
            <span>creative library</span>
          </div>
          <button className="icon-button sidebar-close" onClick={onClose} aria-label="关闭导航">
            <Icon name="close" />
          </button>
        </div>

        <button className="primary-action" onClick={onPrimaryAction} aria-label="添加到素材库">
          <Icon name="plus" size={17} />
          <span>添加到素材库</span>
          <kbd>⌘ N</kbd>
        </button>

        <nav className="side-nav" aria-label="主导航">
          <p className="nav-label">工作区</p>
          {navigation.map((item) => {
            const badge = getBadge(item.key) ?? item.badge;
            return (
              <button
                key={item.key}
                className={current === item.key ? "is-active" : ""}
                aria-label={item.label}
                title={collapsed ? item.label : undefined}
                onClick={() => {
                  onNavigate(item.key);
                  onClose();
                }}
              >
                <Icon name={item.icon} size={18} />
                <span>{item.label}</span>
                {typeof badge === "number" && <em>{badge}</em>}
              </button>
            );
          })}
        </nav>

        <div className="sidebar-spacer" />

        <div className="local-storage-card">
          <div className="storage-icon"><Icon name="database" size={16} /></div>
          <div>
            <strong>Local Library</strong>
            <span>SQLite · 本地媒体</span>
          </div>
          <span className="status-dot" title="存储可用" />
        </div>

        <button className="settings-row" type="button" onClick={onOpenSettings} aria-label="个人空间设置" title={collapsed ? "个人空间设置" : undefined}>
          <span className="avatar">P</span>
          <span><strong>个人空间</strong><small>所有数据仅在此设备</small></span>
          <Icon name="settings" size={17} />
        </button>
      </aside>
    </>
  );
}
