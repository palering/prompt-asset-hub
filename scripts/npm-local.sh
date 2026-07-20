#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p \
  "${project_root}/target/tool-cache/npm" \
  "${project_root}/target/tool-cache/cargo" \
  "${project_root}/.local/tmp"

export npm_config_cache="${project_root}/target/tool-cache/npm"
export CARGO_HOME="${project_root}/target/tool-cache/cargo"
export CARGO_TARGET_DIR="${project_root}/target"
export TMPDIR="${project_root}/.local/tmp"

cd "${project_root}/apps/web"
exec npm "$@"
