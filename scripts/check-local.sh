#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Tauri embeds the built frontend during Rust compilation.
bash "${project_root}/scripts/npm-local.sh" run build
bash "${project_root}/scripts/cargo-local.sh" fmt --all -- --check
bash "${project_root}/scripts/cargo-local.sh" check --workspace
bash "${project_root}/scripts/cargo-local.sh" clippy --workspace --all-targets -- -D warnings
bash "${project_root}/scripts/cargo-local.sh" test --workspace
