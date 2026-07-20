# Prompt Asset Hub

AI 生成式创作资产库：将创作意图、跨模型 Prompt、图片/视频结果与复盘经验连接起来，并以受控工具提供给 AI Agent 复用。

当前公开仓库包含 Local Desktop MVP 的实现代码。产品规划、过程记录与内部设计文档暂不随公开仓库发布。

## 本地开发

项目的开发缓存、数据库、媒体和临时文件均限制在仓库目录内。使用仓库 wrapper 执行命令：

```bash
bash scripts/npm-local.sh install
bash scripts/cargo-local.sh check --workspace
bash scripts/check-local.sh
```

开发缓存、数据库、媒体、浏览器状态与临时文件均由 wrapper 限制在被 Git 忽略的 `target/` 和 `.local/` 目录。

## 产品目标

- 保存并检索 Prompt、图片、视频与参考素材。
- 保留同一创作意图在不同模型上的适配版本及效果差异。
- 支持从任一素材回溯到所用 Prompt、模型、参数和评价。
- 通过 MCP/API 向其他 Agent 提供可控的检索、推荐与写入能力。

## 非目标（MVP）

- 不在第一阶段替代各个生成平台的完整创作工作台。
- 不在第一阶段实现模型 API 直连、协作审批流或复杂版权交易。
- 不在第一阶段提供团队成员、角色或共享空间管理；初版是个人库。
