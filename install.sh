#!/bin/bash
set -euo pipefail

# Claude Keyboard - macOS 安装脚本
# 用法: curl -fsSL https://raw.githubusercontent.com/DurioLab/claude-keyboard/main/install.sh | bash

REPO="DurioLab/claude-keyboard"
APP_NAME="Claude Keyboard"
INSTALL_DIR="/Applications"

# 颜色
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[✗]${NC} $1"; exit 1; }

# 仅支持 macOS
[[ "$(uname)" == "Darwin" ]] || error "此脚本仅支持 macOS"

# 检测架构
ARCH="$(uname -m)"
case "$ARCH" in
  arm64)  DMG_PATTERN="aarch64.dmg" ;;
  x86_64) DMG_PATTERN="x86_64.dmg"  ;;
  *)      error "不支持的架构: $ARCH" ;;
esac

# GitHub API 地址（支持镜像回退）
GH_API="https://api.github.com"
GH_PROXY=""

# 获取最新 release（自动回退镜像）
info "正在获取最新版本..."
RELEASE_JSON=$(curl -fsSL --connect-timeout 10 "${GH_API}/repos/${REPO}/releases/latest" 2>/dev/null) || true

if [[ -z "$RELEASE_JSON" ]] || echo "$RELEASE_JSON" | grep -q '"message"'; then
  warn "GitHub API 不可用，尝试镜像..."
  GH_PROXY="https://ghfast.top/"
  RELEASE_JSON=$(curl -fsSL --connect-timeout 10 "${GH_PROXY}https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null) || true
fi

if [[ -z "$RELEASE_JSON" ]] || echo "$RELEASE_JSON" | grep -q '"message"'; then
  echo ""
  error "无法获取 release 信息。如果你在国内，请设置代理后重试:\n  export https_proxy=http://127.0.0.1:7890\n  然后重新运行安装脚本"
fi

VERSION=$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
DMG_URL=$(echo "$RELEASE_JSON" | grep '"browser_download_url"' | grep "$DMG_PATTERN" | head -1 | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')

# 如果使用镜像，给下载 URL 也加上代理前缀
if [[ -n "$GH_PROXY" ]] && [[ -n "$DMG_URL" ]]; then
  DMG_URL="${GH_PROXY}${DMG_URL}"
fi

[[ -n "$VERSION" ]]  || error "无法解析版本号"
[[ -n "$DMG_URL" ]]  || error "未找到适用于 ${ARCH} 的 DMG 文件"

info "版本: ${VERSION} (${ARCH})"

# 如果已安装，先关闭
if pgrep -x "claude-virtual-keyboard" > /dev/null 2>&1; then
  warn "正在关闭运行中的 ${APP_NAME}..."
  pkill -x "claude-virtual-keyboard" 2>/dev/null || true
  sleep 1
fi

# 下载 DMG
TMPDIR_PATH=$(mktemp -d)
DMG_PATH="${TMPDIR_PATH}/${APP_NAME}.dmg"
trap 'rm -rf "$TMPDIR_PATH"' EXIT

info "正在下载 ${DMG_URL##*/}..."
if ! curl -fSL --progress-bar --http1.1 --connect-timeout 15 -o "$DMG_PATH" "$DMG_URL" 2>/dev/null; then
  # 直连失败，尝试镜像下载
  if [[ -z "$GH_PROXY" ]]; then
    warn "直连下载失败，尝试镜像..."
    MIRROR_URL="https://ghfast.top/${DMG_URL}"
    curl -fSL --progress-bar --connect-timeout 15 -o "$DMG_PATH" "$MIRROR_URL" \
      || error "下载失败，请设置代理后重试:\n  export https_proxy=http://127.0.0.1:7890"
  else
    error "下载失败，请设置代理后重试:\n  export https_proxy=http://127.0.0.1:7890"
  fi
fi

# 校验下载文件是否为有效 DMG
FILE_TYPE=$(file -b "$DMG_PATH" 2>/dev/null || true)
if ! echo "$FILE_TYPE" | grep -qi "compressed\|zlib\|bzip2\|disk image\|Apple"; then
  warn "下载的文件可能不是有效的 DMG (类型: ${FILE_TYPE})"
  warn "如果使用镜像下载，可能获取到了重定向页面"
  error "请尝试设置代理后重试:\n  export https_proxy=http://127.0.0.1:7890"
fi

# 挂载 DMG
info "正在安装..."
MOUNT_OUTPUT=$(hdiutil attach "$DMG_PATH" -nobrowse 2>&1) \
  || error "DMG 挂载失败:\n${MOUNT_OUTPUT}"
MOUNT_POINT=$(echo "$MOUNT_OUTPUT" | grep '/Volumes/' | sed 's/.*\(\/Volumes\/.*\)/\1/' | head -1)
[[ -d "$MOUNT_POINT" ]] || error "DMG 挂载失败: 无法解析挂载点"

# 拷贝到 /Applications
APP_SRC=$(find "$MOUNT_POINT" -maxdepth 1 -name "*.app" | head -1)
[[ -n "$APP_SRC" ]] || { hdiutil detach "$MOUNT_POINT" -quiet; error "DMG 中未找到 .app"; }

rm -rf "${INSTALL_DIR}/${APP_NAME}.app"
cp -R "$APP_SRC" "${INSTALL_DIR}/"

# 卸载 DMG
hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true

# 移除 quarantine（关键步骤）
find "${INSTALL_DIR}/${APP_NAME}.app" -exec xattr -c {} + 2>/dev/null || true

info "安装完成! ${APP_NAME} ${VERSION}"
echo ""
echo "  启动: open \"${INSTALL_DIR}/${APP_NAME}.app\""
echo ""

# 询问是否立即启动
read -r -p "是否立即启动? [Y/n] " answer < /dev/tty 2>/dev/null || answer="n"
case "$answer" in
  [nN]*) info "你可以稍后手动启动" ;;
  *)     open "${INSTALL_DIR}/${APP_NAME}.app"; info "已启动 ${APP_NAME}" ;;
esac
