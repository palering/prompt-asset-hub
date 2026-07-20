import { useEffect } from "react";
import type { AppSettings, ClientMode, HealthStatus } from "../lib/library-client";
import type { DisplayMode } from "./AssetLibrary";
import { Icon } from "./Icon";

interface SettingsDialogProps {
  open: boolean;
  health: HealthStatus | null;
  settings: AppSettings | null;
  clientMode: ClientMode;
  saving: boolean;
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  displayMode: DisplayMode;
  onSidebarCollapsedChange: (collapsed: boolean) => void;
  onSidebarWidthChange: (width: number) => void;
  onDisplayModeChange: (mode: DisplayMode) => void;
  onChooseDataRoot: () => void;
  onReset: () => void;
  onClose: () => void;
}

export function SettingsDialog({
  open,
  health,
  settings,
  clientMode,
  saving,
  sidebarCollapsed,
  sidebarWidth,
  displayMode,
  onSidebarCollapsedChange,
  onSidebarWidthChange,
  onDisplayModeChange,
  onChooseDataRoot,
  onReset,
  onClose,
}: SettingsDialogProps) {
  useEffect(() => {
    if (!open) return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, open]);

  if (!open) return null;

  return (
    <div className="modal-layer settings-layer" role="dialog" aria-modal="true" aria-label="个人空间设置">
      <button className="modal-scrim" type="button" onClick={onClose} aria-label="关闭设置" />
      <section className="settings-dialog">
        <header>
          <div>
            <span className="eyebrow">Personal workspace</span>
            <h2>个人空间设置</h2>
            <p>这些偏好只保存在当前设备，不会修改素材数据。</p>
          </div>
          <button className="icon-button" type="button" onClick={onClose} aria-label="关闭设置">
            <Icon name="close" />
          </button>
        </header>

        <div className="settings-section">
          <div className="settings-section-heading">
            <span className="settings-section-icon"><Icon name="sliders" size={16} /></span>
            <div><strong>默认展示</strong><small>作为下次启动时的工作区默认布局</small></div>
          </div>

          <div className="settings-control-row">
            <div><strong>默认侧栏状态</strong><small>需要更多画布空间时可以收起</small></div>
            <div className="settings-segmented" role="group" aria-label="侧栏状态">
              <button
                type="button"
                className={!sidebarCollapsed ? "is-active" : ""}
                onClick={() => onSidebarCollapsedChange(false)}
              >展开</button>
              <button
                type="button"
                className={sidebarCollapsed ? "is-active" : ""}
                onClick={() => onSidebarCollapsedChange(true)}
              >收起</button>
            </div>
          </div>

          <div className={`settings-control-row range-row ${sidebarCollapsed ? "is-disabled" : ""}`}>
            <div><strong>默认侧栏宽度</strong><small>也可以直接拖动侧栏右边缘</small></div>
            <label className="sidebar-range">
              <input
                type="range"
                min="210"
                max="360"
                step="1"
                value={sidebarWidth}
                disabled={sidebarCollapsed}
                onChange={(event) => onSidebarWidthChange(Number(event.target.value))}
              />
              <output>{Math.round(sidebarWidth)} px</output>
            </label>
          </div>

          <div className="settings-control-row">
            <div><strong>素材默认布局</strong><small>设置素材库默认使用的布局</small></div>
            <div className="settings-segmented icon-segmented" role="group" aria-label="素材展示方式">
              <button
                type="button"
                className={displayMode === "card" ? "is-active" : ""}
                onClick={() => onDisplayModeChange("card")}
              ><Icon name="grid" size={14} /> 卡片</button>
              <button
                type="button"
                className={displayMode === "list" ? "is-active" : ""}
                onClick={() => onDisplayModeChange("list")}
              ><Icon name="list" size={14} /> 列表</button>
            </div>
          </div>
        </div>

        <div className="settings-section storage-settings">
          <div className="settings-section-heading">
            <span className="settings-section-icon"><Icon name="database" size={16} /></span>
            <div><strong>本地存储</strong><small>数据库和媒体共用一个数据根目录</small></div>
          </div>

          <div className="storage-runtime-line">
            <span>{clientMode === "tauri" ? "Tauri 桌面端" : "浏览器只读示例"}</span>
            <span>{settings?.storage.backend ?? health?.database ?? "—"}</span>
            <span>{settings?.storage.source === "environment" ? "环境变量覆盖" : "config.toml"}</span>
            <strong className="healthy"><i /> {health?.status === "degraded" ? "需要检查" : "连接正常"}</strong>
          </div>

          <div className="data-root-control">
            <div>
              <span>当前数据根目录</span>
              <code>{settings?.storage.active_data_root ?? "正在读取…"}</code>
            </div>
            <button
              className="button"
              type="button"
              disabled={clientMode !== "tauri" || saving}
              onClick={onChooseDataRoot}
            >
              <Icon name="folder" size={14} /> {saving ? "保存中…" : "选择目录"}
            </button>
          </div>

          {settings?.storage.restart_required && (
            <div className="restart-required-note">
              <Icon name="activity" size={14} />
              <span>
                <strong>新目录将在重启后生效</strong>
                <code>{settings.storage.configured_data_root}</code>
              </span>
            </div>
          )}

          <div className="storage-path-list">
            <div><span>配置文件</span><code>{settings?.config_path ?? "—"}</code></div>
            <div><span>数据库文件</span><code>{settings?.storage.database_path ?? "—"}</code></div>
            <div><span>媒体与缩略图</span><code>{settings?.storage.media_path ?? "—"}</code></div>
            <div><span>导出目录</span><code>{settings?.storage.exports_path ?? "—"}</code></div>
            <div><span>运行临时目录</span><code>{settings?.storage.temp_path ?? "—"}</code></div>
          </div>

          <p className="storage-scope-note">
            <Icon name="check" size={13} />
            选择目录只更新配置，不自动移动现有数据库或媒体；macOS 可能请求访问所选文件夹。
          </p>
        </div>

        <footer>
          <button className="button settings-reset" type="button" onClick={onReset}>恢复默认布局</button>
          <button className="button primary" type="button" onClick={onClose}>完成</button>
        </footer>
      </section>
    </div>
  );
}
