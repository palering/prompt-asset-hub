import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { AssetLibrary, type DisplayMode } from "./components/AssetLibrary";
import {
  CreateProjectDialog,
  CreatePromptDialog,
  CreateVariantDialog,
  ImportDialog,
} from "./components/EntityDialogs";
import { Icon } from "./components/Icon";
import { PromptWorkbench } from "./components/PromptWorkbench";
import { ProjectsView } from "./components/ProjectsView";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar, type ViewKey } from "./components/Sidebar";
import { createLibraryClient } from "./lib/create-library-client";
import {
  emptyBootstrap,
  type AppSettings,
  type AssetDetail,
  type AssetQuery,
  type AssetSummary,
  type CreatePromptInput,
  type HealthStatus,
  type ImportAssetOptions,
  type LibraryBootstrap,
  type PromptSummary,
  type PromptVariantInput,
  type UpdateAssetContextInput,
  type UpdateAssetTagsInput,
} from "./lib/library-client";

const client = createLibraryClient();

function mergeKnownTags(current: string[], assets: AssetSummary[]): string[] {
  const tags = new Map(current.map((tag) => [tag.toLocaleLowerCase(), tag]));
  for (const asset of assets) {
    for (const tag of asset.tags) tags.set(tag.toLocaleLowerCase(), tag);
  }
  return [...tags.values()].sort((left, right) => left.localeCompare(right, "zh-CN"));
}
const loadAssetDetail = (assetId: string) => client.getAssetDetail(assetId);
const SIDEBAR_WIDTH_STORAGE_KEY = "prompt-asset-hub.sidebar-width";
const SIDEBAR_COLLAPSED_STORAGE_KEY = "prompt-asset-hub.sidebar-collapsed";
const DISPLAY_MODE_STORAGE_KEY = "prompt-asset-hub.library-display-mode";
const DEFAULT_SIDEBAR_WIDTH = 238;

function readStoredSidebarWidth(): number {
  try {
    const value = Number(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY));
    return Number.isFinite(value) && value >= 210 && value <= 360 ? value : DEFAULT_SIDEBAR_WIDTH;
  } catch {
    return DEFAULT_SIDEBAR_WIDTH;
  }
}

function readStoredBoolean(key: string): boolean {
  try {
    return window.localStorage.getItem(key) === "true";
  } catch {
    return false;
  }
}

function readStoredDisplayMode(): DisplayMode {
  try {
    return window.localStorage.getItem(DISPLAY_MODE_STORAGE_KEY) === "list" ? "list" : "card";
  } catch {
    return "card";
  }
}

const viewLabels: Record<ViewKey, string> = {
  library: "素材库",
  prompts: "Prompt 工作台",
  projects: "项目",
  activity: "Agent 记录",
};

function SecondaryView({ onAction }: { onAction: () => void }) {
  return (
    <div className="view-shell secondary-view">
      <section className="view-intro">
        <div><h1>{viewLabels.activity}</h1><p>查看 Agent 如何读取、引用与写入你的创作库。</p></div>
        <button className="button primary" onClick={onAction}><Icon name="plus" size={16} /> 连接 Agent</button>
      </section>

      <div className="secondary-placeholder">
        <div className="placeholder-visual agent">
          <span><Icon name="command" size={28} /></span>
          <i /><i /><i />
        </div>
        <span className="eyebrow">Local MVP · 下一阶段</span>
        <h2>受控地开放给你的 Agent</h2>
        <p>MCP 会先提供只读检索，所有写入都需要确认并留下调用记录。</p>
        <button className="button" onClick={onAction}>查看接入规划 <Icon name="arrow" size={15} /></button>
      </div>
    </div>
  );
}

export default function App() {
  const [currentView, setCurrentView] = useState<ViewKey>("library");
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => readStoredBoolean(SIDEBAR_COLLAPSED_STORAGE_KEY));
  const [sidebarWidth, setSidebarWidth] = useState(readStoredSidebarWidth);
  const [displayMode, setDisplayMode] = useState<DisplayMode>(readStoredDisplayMode);
  const [bootstrap, setBootstrap] = useState<LibraryBootstrap | null>(null);
  const [appSettings, setAppSettings] = useState<AppSettings | null>(null);
  const [health, setHealth] = useState<HealthStatus | null>(null);
  const [assets, setAssets] = useState<AssetSummary[]>([]);
  const [knownTags, setKnownTags] = useState<string[]>([]);
  const [total, setTotal] = useState(0);
  const [query, setQuery] = useState<AssetQuery>({ media_type: "all", sort: "newest" });
  const [loading, setLoading] = useState(true);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [projectDialogOpen, setProjectDialogOpen] = useState(false);
  const [promptDialogOpen, setPromptDialogOpen] = useState(false);
  const [variantPrompt, setVariantPrompt] = useState<PromptSummary | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [writing, setWriting] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const settingsLoadedRef = useRef(false);

  const showToast = (message: string) => {
    setToast(message);
    window.setTimeout(() => setToast(null), 2600);
  };

  useEffect(() => {
    let active = true;
    const initialize = async () => {
      setLoading(true);
      try {
        const [healthResult, bootstrapResult, page, settingsResult] = await Promise.all([
          client.health(),
          client.bootstrap(),
          client.listAssets(),
          client.getAppSettings(),
        ]);
        if (!active) return;
        setHealth(healthResult);
        setBootstrap(bootstrapResult);
        setAssets(page.items);
        setKnownTags(mergeKnownTags([], page.items));
        setTotal(page.total);
        setAppSettings(settingsResult);
        if (client.mode === "tauri") {
          setSidebarCollapsed(settingsResult.ui.sidebar_collapsed);
          setSidebarWidth(settingsResult.ui.sidebar_width);
          setDisplayMode(settingsResult.ui.default_asset_view);
        }
        settingsLoadedRef.current = true;
      } catch (reason) {
        if (!active) return;
        setBootstrap(emptyBootstrap(client.mode === "mock" ? "demo" : "local"));
        setError(reason instanceof Error ? reason.message : "本地素材库暂时无法连接");
      } finally {
        if (active) setLoading(false);
      }
    };
    void initialize();
    return () => { active = false; };
  }, []);

  useEffect(() => {
    try {
      window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(sidebarWidth));
      window.localStorage.setItem(SIDEBAR_COLLAPSED_STORAGE_KEY, String(sidebarCollapsed));
      window.localStorage.setItem(DISPLAY_MODE_STORAGE_KEY, displayMode);
    } catch {
      // UI preferences remain usable for the current session.
    }
  }, [displayMode, sidebarCollapsed, sidebarWidth]);

  useEffect(() => {
    if (client.mode !== "tauri" || !settingsLoadedRef.current) return undefined;
    const timer = window.setTimeout(() => {
      setSettingsSaving(true);
      void client.updateAppSettings({
        ui: {
          default_asset_view: displayMode,
          sidebar_collapsed: sidebarCollapsed,
          sidebar_width: Math.round(sidebarWidth),
        },
      })
        .then(setAppSettings)
        .catch((reason) => {
          setError(reason instanceof Error ? reason.message : String(reason));
        })
        .finally(() => setSettingsSaving(false));
    }, 220);
    return () => window.clearTimeout(timer);
  }, [displayMode, sidebarCollapsed, sidebarWidth]);

  useEffect(() => {
    if (loading) return;
    let active = true;
    const timer = window.setTimeout(async () => {
      setSearching(true);
      try {
        const page = query.query?.trim()
          ? await client.searchLibrary(query)
          : await client.listAssets(query);
        if (!active) return;
        setAssets(page.items);
        setKnownTags((current) => mergeKnownTags(current, page.items));
        setTotal(page.total);
        setError(null);
      } catch (reason) {
        if (active) setError(reason instanceof Error ? reason.message : "搜索失败，请稍后重试");
      } finally {
        if (active) setSearching(false);
      }
    }, 180);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [loading, query]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLocaleLowerCase() === "n") {
        event.preventDefault();
        setImportOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const resolvedBootstrap = bootstrap ?? emptyBootstrap(client.mode === "mock" ? "demo" : "local");
  const counts = useMemo(() => ({
    assets: bootstrap?.counts.assets ?? assets.length,
    prompts: bootstrap?.counts.prompts ?? bootstrap?.prompts.length ?? 0,
  }), [assets.length, bootstrap]);
  const shellStyle = {
    "--sidebar-width": `${sidebarCollapsed ? 68 : sidebarWidth}px`,
    "--sidebar-expanded-width": `${sidebarWidth}px`,
  } as CSSProperties;

  const refreshLibrary = async () => {
    const [page, nextBootstrap] = await Promise.all([client.listAssets(query), client.bootstrap()]);
    setAssets(page.items);
    setKnownTags((current) => mergeKnownTags(current, page.items));
    setTotal(page.total);
    setBootstrap(nextBootstrap);
  };

  const requireDesktop = (action: string): boolean => {
    if (client.mode === "tauri") return true;
    showToast(`浏览器示例为只读，请在 Tauri 桌面端${action}`);
    return false;
  };

  const handleFileSelection = async (files: File[], options: ImportAssetOptions) => {
    if (files.length === 0) return;
    if (client.mode === "mock") {
      setImportOpen(false);
      showToast("浏览器示例不会写入文件，请在 Tauri 桌面端导入");
      return;
    }
    setWriting(true);
    try {
      const imported = await client.importAssets(files, options);
      await refreshLibrary();
      setError(null);
      setImportOpen(false);
      showToast(options.variant_id
        ? `已导入 ${imported.length} 份素材并建立生成链路`
        : `已导入 ${imported.length} 份素材并生成缩略图`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      showToast("导入失败，请检查文件格式");
    } finally {
      setWriting(false);
    }
  };

  const handleCreateProject = async (input: { name: string; description?: string }) => {
    setWriting(true);
    try {
      await client.createProject(input);
      await refreshLibrary();
      setProjectDialogOpen(false);
      showToast("项目已创建");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWriting(false);
    }
  };

  const handleCreatePrompt = async (input: CreatePromptInput) => {
    setWriting(true);
    try {
      await client.createPrompt(input);
      await refreshLibrary();
      setPromptDialogOpen(false);
      showToast(input.initial_variant ? "Prompt 与首个模型变体已创建" : "Prompt 母版已创建");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWriting(false);
    }
  };

  const handleCreateVariant = async (promptId: string, input: PromptVariantInput) => {
    setWriting(true);
    try {
      await client.addPromptVariant(promptId, input);
      await refreshLibrary();
      setVariantPrompt(null);
      showToast("模型变体已添加");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWriting(false);
    }
  };

  const handleUpdateAssetContext = async (input: UpdateAssetContextInput): Promise<AssetDetail> => {
    if (client.mode === "mock") {
      throw new Error("浏览器示例为只读，请在 Tauri 桌面端编辑素材资料");
    }
    setWriting(true);
    try {
      const detail = await client.updateAssetContext(input);
      await refreshLibrary();
      setError(null);
      showToast("素材资料已更新，旧生成快照已保留");
      return detail;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setError(message);
      throw reason;
    } finally {
      setWriting(false);
    }
  };

  const handleUpdateAssetTags = async (input: UpdateAssetTagsInput): Promise<AssetDetail> => {
    if (client.mode === "mock") {
      throw new Error("浏览器示例为只读，请在 Tauri 桌面端编辑素材标签");
    }
    setWriting(true);
    try {
      const detail = await client.updateAssetTags(input);
      setKnownTags((current) => mergeKnownTags(current, [{
        id: input.asset_id,
        title: "",
        media_type: "image",
        tags: detail.tags,
        created_at: detail.created_at,
      }]));
      await refreshLibrary();
      setError(null);
      showToast("素材标签已更新");
      return detail;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setError(message);
      throw reason;
    } finally {
      setWriting(false);
    }
  };

  const handleChooseDataRoot = async () => {
    if (client.mode !== "tauri") {
      showToast("浏览器示例不能修改存储目录");
      return;
    }
    setSettingsSaving(true);
    try {
      const nextSettings = await client.chooseDataRoot();
      if (!nextSettings) return;
      setAppSettings(nextSettings);
      showToast(nextSettings.storage.restart_required
        ? "新数据目录已保存，重启应用后生效"
        : "当前数据目录保持不变");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      showToast("无法保存数据目录");
    } finally {
      setSettingsSaving(false);
    }
  };

  return (
    <div className={`app-shell ${sidebarCollapsed ? "sidebar-is-collapsed" : ""}`} style={shellStyle}>
      <Sidebar
        current={currentView}
        assetCount={counts.assets}
        promptCount={counts.prompts}
        open={sidebarOpen}
        collapsed={sidebarCollapsed}
        onNavigate={setCurrentView}
        onClose={() => setSidebarOpen(false)}
        onToggleCollapsed={() => setSidebarCollapsed((value) => !value)}
        onResize={setSidebarWidth}
        onPrimaryAction={() => setImportOpen(true)}
        onOpenSettings={() => {
          setSidebarOpen(false);
          setSettingsOpen(true);
        }}
      />

      <main className="app-main">
        <header className="topbar">
          <button className="icon-button menu-button" onClick={() => setSidebarOpen(true)} aria-label="打开导航"><Icon name="menu" /></button>
          <div className="breadcrumbs"><span>{resolvedBootstrap.library_name}</span><Icon name="chevron" size={12} /><strong>{viewLabels[currentView]}</strong></div>
          <div className="topbar-spacer" />
          {error ? (
            <button className="connection-pill is-error" onClick={() => window.location.reload()}><span /> 重新连接</button>
          ) : (
            <span className="connection-pill"><span /> {client.mode === "tauri" ? "本地库已连接" : "浏览器示例"}</span>
          )}
          <button className="icon-button topbar-command" aria-label="打开命令面板"><Icon name="command" size={17} /></button>
        </header>

        {error && (
          <div className="error-banner"><Icon name="database" size={16} /><span>{error}</span><small>{health?.status ?? "offline"}</small></div>
        )}

        {currentView === "library" && (
          <AssetLibrary
            assets={assets}
            total={total}
            models={resolvedBootstrap.models}
            query={query}
            loading={loading}
            refreshing={searching}
            displayMode={displayMode}
            isDemo={client.mode === "mock"}
            prompts={resolvedBootstrap.prompts}
            availableTags={knownTags}
            writing={writing}
            onQueryChange={setQuery}
            onDisplayModeChange={setDisplayMode}
            onImport={() => setImportOpen(true)}
            onLoadDetail={loadAssetDetail}
            onUpdateAssetContext={handleUpdateAssetContext}
            onUpdateAssetTags={handleUpdateAssetTags}
          />
        )}
        {currentView === "prompts" && (
          <PromptWorkbench
            prompts={resolvedBootstrap.prompts}
            loading={loading}
            isDemo={client.mode === "mock"}
            onCreate={() => {
              if (requireDesktop("创建 Prompt")) setPromptDialogOpen(true);
            }}
            onAddVariant={(prompt) => {
              if (requireDesktop("添加模型变体")) setVariantPrompt(prompt);
            }}
          />
        )}
        {currentView === "projects" && (
          <ProjectsView
            projects={resolvedBootstrap.projects}
            prompts={resolvedBootstrap.prompts}
            loading={loading}
            isDemo={client.mode === "mock"}
            onCreate={() => {
              if (requireDesktop("创建项目")) setProjectDialogOpen(true);
            }}
            onOpenPrompts={() => setCurrentView("prompts")}
          />
        )}
        {currentView === "activity" && (
          <SecondaryView onAction={() => showToast("只读 MCP 接入将在下一里程碑实现")} />
        )}
      </main>

      <ImportDialog
        open={importOpen}
        prompts={resolvedBootstrap.prompts}
        busy={writing}
        storagePath={appSettings?.storage.media_path}
        storageRestartPending={appSettings?.storage.restart_required}
        onClose={() => setImportOpen(false)}
        onSubmit={handleFileSelection}
      />
      <CreateProjectDialog
        open={projectDialogOpen}
        busy={writing}
        onClose={() => setProjectDialogOpen(false)}
        onSubmit={handleCreateProject}
      />
      <CreatePromptDialog
        open={promptDialogOpen}
        projects={resolvedBootstrap.projects}
        busy={writing}
        onClose={() => setPromptDialogOpen(false)}
        onNeedProject={() => {
          setPromptDialogOpen(false);
          setProjectDialogOpen(true);
        }}
        onSubmit={handleCreatePrompt}
      />
      <CreateVariantDialog
        open={Boolean(variantPrompt)}
        prompt={variantPrompt}
        busy={writing}
        onClose={() => setVariantPrompt(null)}
        onSubmit={handleCreateVariant}
      />
      <SettingsDialog
        open={settingsOpen}
        health={health}
        settings={appSettings}
        clientMode={client.mode}
        saving={settingsSaving}
        sidebarCollapsed={sidebarCollapsed}
        sidebarWidth={sidebarWidth}
        displayMode={displayMode}
        onSidebarCollapsedChange={setSidebarCollapsed}
        onSidebarWidthChange={setSidebarWidth}
        onDisplayModeChange={setDisplayMode}
        onChooseDataRoot={handleChooseDataRoot}
        onReset={() => {
          setSidebarCollapsed(false);
          setSidebarWidth(DEFAULT_SIDEBAR_WIDTH);
          setDisplayMode("card");
        }}
        onClose={() => setSettingsOpen(false)}
      />
      {toast && <div className="toast"><Icon name="check" size={15} /> {toast}</div>}
    </div>
  );
}
