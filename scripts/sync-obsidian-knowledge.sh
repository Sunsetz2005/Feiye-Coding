#!/usr/bin/env bash
# Markdown code spans intentionally use backticks in single-quoted printf formats.
# shellcheck disable=SC2016
set -eu

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
vault_root="${SUNSETZ_OBSIDIAN_VAULT:-/Users/a10954/Documents/Documents - Sunsetz的MacBook Pro/Obsidian Vault}"
project_dir="$vault_root/Sunsetz"
sync_dir="$project_dir/自动同步"
generated_at="$(date '+%Y-%m-%dT%H:%M:%S%z')"

mkdir -p "$sync_dir"
cd "$repo_root"

branch="$(git branch --show-current 2>/dev/null || true)"
if [ -z "$branch" ]; then
  branch="(detached HEAD)"
fi
head_full="$(git rev-parse HEAD)"
head_short="$(git rev-parse --short HEAD)"
commit_count="$(git rev-list --count HEAD)"
version="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' package.json | head -n 1)"
remote_url="$(git remote get-url origin 2>/dev/null || true)"
remote_url="$(printf '%s' "$remote_url" | sed -E 's#(https?://)[^/@]+:[^/@]+@#\1<redacted>@#')"
status_lines="$(git status --short)"
if [ -z "$status_lines" ]; then
  worktree_state="clean"
else
  worktree_state="dirty"
fi

upstream="$(git rev-parse --abbrev-ref '@{upstream}' 2>/dev/null || true)"
ahead="unknown"
behind="unknown"
if [ -n "$upstream" ]; then
  counts="$(git rev-list --left-right --count "$upstream...HEAD")"
  behind="$(printf '%s' "$counts" | awk '{print $1}')"
  ahead="$(printf '%s' "$counts" | awk '{print $2}')"
fi

target="$sync_dir/仓库状态.md"
tmp="$(mktemp "$sync_dir/.仓库状态.XXXXXX")"
{
  printf '%s\n' '---'
  printf '%s\n' 'title: 仓库状态（自动生成）'
  printf '%s\n' 'project: Sunsetz'
  printf '%s\n' 'type: generated-status'
  printf 'updated: "%s"\n' "$generated_at"
  printf '%s\n' 'tags:' '  - project/sunsetz-desktop' '  - generated' '---' '' '# 仓库状态（自动生成）' ''
  printf '> [!info] 生成边界\n> 本页由 `%s` 生成。请勿手工编辑；服务器密钥和运行时 Secret 不在采集范围。\n\n' 'scripts/sync-obsidian-knowledge.sh'
  printf '| 字段 | 当前值 |\n|---|---|\n'
  printf '| 生成时间 | `%s` |\n' "$generated_at"
  printf '| 仓库路径 | `%s` |\n' "$repo_root"
  printf '| 版本 | `%s` |\n' "$version"
  printf '| 分支 | `%s` |\n' "$branch"
  printf '| HEAD | `%s` |\n' "$head_full"
  printf '| 提交总数 | `%s` |\n' "$commit_count"
  printf '| 上游 | `%s` |\n' "${upstream:-未配置}"
  printf '| 相对上游 | ahead `%s` / behind `%s` |\n' "$ahead" "$behind"
  printf '| 工作树 | `%s` |\n' "$worktree_state"
  printf '| origin | `%s` |\n\n' "${remote_url:-未配置}"
  printf '## 未提交改动\n\n'
  if [ -z "$status_lines" ]; then
    printf '%s\n' '- 无。'
  else
    printf '%s\n' "$status_lines" | sed 's/^/- `/' | sed 's/$/`/'
  fi
  printf '\n## 阅读顺序\n\n1. [[../00-AI接手总览|AI 接手总览]]\n2. [[../01-当前状态|当前状态]]\n3. [[../03-未来开发规划|未来开发规划]]\n4. [[权威进度镜像]]\n'
} > "$tmp"
mv "$tmp" "$target"

target="$sync_dir/最近提交.md"
tmp="$(mktemp "$sync_dir/.最近提交.XXXXXX")"
{
  printf '%s\n' '---'
  printf '%s\n' 'title: 最近提交（自动生成）'
  printf '%s\n' 'project: Sunsetz'
  printf '%s\n' 'type: generated-history'
  printf 'updated: "%s"\n' "$generated_at"
  printf '%s\n' 'tags:' '  - project/sunsetz-desktop' '  - generated' '---' '' '# 最近提交（自动生成）' ''
  printf '> [!info]\n> 来源是本地 Git 历史，当前 HEAD 为 `%s`。\n\n' "$head_short"
  printf '| 提交 | 日期 | 说明 |\n|---|---|---|\n'
  git log -30 --format='%h%x09%ad%x09%s' --date=short | while IFS="$(printf '\t')" read -r hash commit_date subject; do
    safe_subject="$(printf '%s' "$subject" | sed 's/|/\\|/g')"
    printf '| `%s` | %s | %s |\n' "$hash" "$commit_date" "$safe_subject"
  done
} > "$tmp"
mv "$tmp" "$target"

mirror_markdown() {
  source_path="$1"
  target_name="$2"
  note_title="$3"
  source_label="$4"
  target_path="$sync_dir/$target_name"
  mirror_tmp="$(mktemp "$sync_dir/.mirror.XXXXXX")"
  {
    printf '%s\n' '---'
    printf 'title: "%s"\n' "$note_title"
    printf '%s\n' 'project: Sunsetz'
    printf '%s\n' 'type: generated-mirror'
    printf 'updated: "%s"\n' "$generated_at"
    printf '%s\n' 'tags:' '  - project/sunsetz-desktop' '  - generated' '---' ''
    printf '> [!warning] 自动镜像\n> 来源：`%s`。本页会被覆盖，请修改仓库源文件。\n\n' "$source_label"
    sed '1{/^---$/d;}' "$source_path"
  } > "$mirror_tmp"
  mv "$mirror_tmp" "$target_path"
}

mirror_markdown "$repo_root/docs/长期重构-执行状态.md" "权威进度镜像.md" "权威进度镜像（自动生成）" "docs/长期重构-执行状态.md"
mirror_markdown "$repo_root/CHANGELOG.md" "变更日志镜像.md" "变更日志镜像（自动生成）" "CHANGELOG.md"
mirror_markdown "$repo_root/docs/llm-wiki/README.md" "AI文档索引镜像.md" "AI 文档索引镜像（自动生成）" "docs/llm-wiki/README.md"

printf 'Sunsetz Obsidian knowledge sync completed: %s\n' "$generated_at"
