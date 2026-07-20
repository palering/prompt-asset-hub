#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p \
  "${project_root}/.local/browser-home" \
  "${project_root}/.local/browser-cache" \
  "${project_root}/.local/browser-config" \
  "${project_root}/.local/qa/screenshots"

export HOME="${project_root}/.local/browser-home"
export XDG_CACHE_HOME="${project_root}/.local/browser-cache"
export XDG_CONFIG_HOME="${project_root}/.local/browser-config"

exec agent-browser "$@"
