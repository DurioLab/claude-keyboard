#!/bin/bash
# Record demo GIF for Claude Keyboard
# Usage: ./record-demo.sh

set -e

OUTPUT_DIR="/Users/mapan/aicoding/claude-virtual-keyboard/docs"
VIDEO_FILE="$OUTPUT_DIR/demo.mov"
GIF_FILE="$OUTPUT_DIR/demo.gif"

echo "=== Claude Keyboard Demo Recorder ==="
echo ""
echo "步骤："
echo "  1. 先确保 Claude Keyboard app 已启动"
echo "  2. 脚本会开始录屏（15秒），你需要选择录制区域"
echo "  3. 选好区域后，立刻在另一个终端运行："
echo "     python3 test_permission.py Bash 'npm install express'"
echo "  4. 在弹出的键盘上用 ← → 切换，Enter 确认"
echo "  5. 15秒后自动停止录制"
echo ""
read -p "准备好了按 Enter 开始录制..."

echo ""
echo ">>> 开始录屏（15秒）— 请用鼠标框选 Claude Keyboard 区域"
echo ">>> 选好区域后，去另一个终端触发权限请求"
echo ""

# Record 15 seconds of video, interactive selection mode
/usr/sbin/screencapture -v -V 15 -x "$VIDEO_FILE"

if [ ! -f "$VIDEO_FILE" ]; then
  echo "❌ 录制失败或被取消"
  exit 1
fi

echo ">>> 录制完成: $VIDEO_FILE"
echo ">>> 正在转换为 GIF..."

# Convert to GIF with ffmpeg
# - Scale to 800px wide
# - 15fps for smooth but small file
# - Generate palette for better quality
PALETTE="/tmp/palette.png"

ffmpeg -y -i "$VIDEO_FILE" \
  -vf "fps=15,scale=800:-1:flags=lanczos,palettegen=stats_mode=diff" \
  "$PALETTE"

ffmpeg -y -i "$VIDEO_FILE" -i "$PALETTE" \
  -lavfi "fps=15,scale=800:-1:flags=lanczos [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
  "$GIF_FILE"

# Clean up
rm -f "$PALETTE"

echo ""
echo "✅ GIF 生成完成: $GIF_FILE"
ls -lh "$GIF_FILE"
echo ""
echo "下一步：告诉 Peter 文件路径，他会帮你更新 README 并 push"
