#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
qa_dir="${project_root}/.local/qa/screenshots"
mkdir -p "${qa_dir}"

export HOME="${project_root}/.local/browser-home"
export XDG_CACHE_HOME="${project_root}/.local/browser-cache"
export XDG_CONFIG_HOME="${project_root}/.local/browser-config"

session="pahqa"
agent-browser --session "${session}" open http://127.0.0.1:1420
agent-browser --session "${session}" wait --load networkidle
agent-browser --session "${session}" screenshot "${qa_dir}/library.png"
agent-browser --session "${session}" snapshot -i

agent-browser --session "${session}" fill @e23 "护肤品"
agent-browser --session "${session}" wait --text "2 份素材"
agent-browser --session "${session}" screenshot "${qa_dir}/search.png"

agent-browser --session "${session}" find text "Prompt 工作台" click
agent-browser --session "${session}" wait --text "Prompt 工作台"
agent-browser --session "${session}" screenshot "${qa_dir}/prompt-workbench.png"

agent-browser --session "${session}" console
agent-browser --session "${session}" errors
agent-browser --session "${session}" close
