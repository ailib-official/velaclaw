#!/bin/bash
# 同步上游 velaclaw 仓库的变更到 velaclaw
# 用法: ./sync-upstream.sh [--dry-run] [--list] [--cherry-pick <commit>]

set -e
cd "$(dirname "$0")"

DRY_RUN=false
LIST_ONLY=false
CHERRY_PICK=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --list)
            LIST_ONLY=true
            shift
            ;;
        --cherry-pick)
            CHERRY_PICK="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done

echo "=== VelaClaw 上游同步 ==="
echo ""

echo "正在获取上游变更..."
git fetch upstream main

UPSTREAM_HASH=$(git rev-parse upstream/main)
LOCAL_HASH=$(git rev-parse main)

if $LIST_ONLY; then
    echo ""
    echo "=== 上游 main 分支最近变更 ==="
    echo ""
    git log --oneline upstream/main | head -50
    echo ""
    echo "查看更多: git log upstream/main --oneline"
    echo "查看特定文件: git log upstream/main -- <文件路径>"
    echo "查看提交详情: git show <commit-hash>"
    exit 0
fi

if [[ -n "$CHERRY_PICK" ]]; then
    echo ""
    echo "=== Cherry-pick 模式 ==="
    echo "正在选择提交: $CHERRY_PICK"
    
    if git cherry-pick "$CHERRY_PICK"; then
        echo ""
        echo "=== Cherry-pick 完成 ==="
        echo "推送到 origin: git push origin main"
    else
        echo ""
        echo "=== Cherry-pick 冲突 ==="
        echo "请手动解决冲突后:"
        echo "  git add <冲突文件>"
        echo "  git cherry-pick --continue"
    fi
    exit 0
fi

if [[ "$UPSTREAM_HASH" == "$LOCAL_HASH" ]]; then
    echo "已与 upstream/main 同步"
    exit 0
fi

COMMITS_BEHIND=$(git rev-list --count main..upstream/main)
COMMITS_AHEAD=$(git rev-list --count upstream/main..main)

echo "上游领先 $COMMITS_BEHIND 个提交"
echo "本地领先 $COMMITS_AHEAD 个提交（你的变更）"

if $DRY_RUN; then
    echo ""
    echo "=== 预览模式 ==="
    echo "将要合并的提交:"
    git log --oneline main..upstream/main | head -30
    if [[ $COMMITS_BEHIND -gt 30 ]]; then
        echo "... 还有 $((COMMITS_BEHIND - 30)) 个提交"
    fi
    echo ""
    echo "不带 --dry-run 参数执行合并"
    exit 0
fi

echo ""
echo "正在合并 upstream/main..."

if git merge upstream/main --no-edit; then
    echo ""
    echo "=== 同步完成 ==="
    echo "已合并 $COMMITS_BEHIND 个上游提交"
    echo ""
    echo "下一步:"
    echo "  1. 查看变更: git log HEAD~$COMMITS_BEHIND..HEAD"
    echo "  2. 推送: git push origin main"
else
    echo ""
    echo "=== 合并冲突 ==="
    echo "检测到冲突，请手动解决:"
    echo "  1. 查看冲突: git status"
    echo "  2. 编辑冲突文件"
    echo "  3. 暂存解决: git add <文件>"
    echo "  4. 完成合并: git commit"
    echo "  5. 推送: git push origin main"
fi
