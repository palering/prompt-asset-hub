#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "${project_root}/target/tool-cache/cargo" "${project_root}/.local/tmp" "${project_root}/target"

export CARGO_HOME="${project_root}/target/tool-cache/cargo"
export CARGO_TARGET_DIR="${project_root}/target"
export TMPDIR="${project_root}/.local/tmp"

exec cargo "$@"
