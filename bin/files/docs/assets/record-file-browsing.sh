#!/usr/bin/env bash
# “大猩猩也会用！”——真实桌面录制脚本（Niri / Wayland）。
# 输出：同目录 file-browsing.gif；原始 MP4 和日志保留在输出提示的临时目录。
# 依赖：niri、wtype、wf-recorder、ffmpeg、python3（evdev 模块及 /dev/uinput 访问权限）；DMS 桌面自动切为浅色。
#
# 录制前：打开独立的 Files 演示窗口，大小 1200×760，完整显示在一个屏幕内。
# 必须移到空白工作区并居中，仅保留 Files；取得用户空闲时段后再运行。
# 使用当前桌面壁纸和系统浅色主题；截取窗口及四周 32 像素桌面。
# Workspace 中按名称升序排列：Documents、Images、Projects 三个文件夹，
# 以及 README.md、notes.md、todo.txt。Images 内放 Big-Sur.jpg、Bliss.jpg、
# Mojave.jpg，图片来源见 theme-sync-sources.md（也可以换成自己的图片）。
# 初态：Workspace 为列表，Images 已设为网格；侧边栏展开，详情面板关闭。
# 请先浏览 Images 一次让缩略图缓存完成，再回到 Workspace，清空筛选。
#
# 分镜（约 50 秒，含按键间隔；键盘驱动真实界面，不合成应用画面）：
#  01：文件夹和文件列表。
#  02：进入 Images，展示三张图片缩略图。
#  03：网格 → 列表 → 网格。
#  04：输入 Bliss 筛选，停顿，再清空筛选。
# 05：选择 Big-Sur.jpg，展开详情页展示预览和元数据。
# 06：折叠侧边栏，停顿，再展开侧边栏。
# 07：折叠详情页，停顿，再展开详情页。
# 08：选择下一张图片，展示详情随选择更新。
# 09：关闭详情，返回 Workspace，前进 Images，再返回开场。
#
# 用法（窗口 ID 可用 niri msg -j windows 查看）：
#   ./record-file-browsing.sh WINDOW_ID OUTPUT_NAME
# 例如：./record-file-browsing.sh 1496 DP-4
# WF_RECORDER 可指定未加入 PATH 的 wf-recorder 可执行文件。
# INPUT_PYTHON 可指定安装了 evdev / evdev-binary 的 Python。
# 初次导出保留原速；用 edit-file-browsing.py 剪去静止等待，成片约 20 秒。
# 剪辑脚本的时间点对应本次录制，重新录制后须核对；不要整体加速。
# 录制期间不要切换窗口或输入；脚本会改变焦点和系统浅色模式。
set -euo pipefail
window_id=${1:?请提供 Files 窗口 ID}
output=${2:?请提供屏幕名称，例如 DP-4}
assets=$(cd -- "$(dirname -- "$0")" && pwd)
work=$(mktemp -d /tmp/gds-file-browsing-XXXXXX)
recorder=${WF_RECORDER:-wf-recorder}
input_python=${INPUT_PYTHON:-python3}
"$input_python" -c "import evdev"
if command -v dms >/dev/null; then dms ipc call theme light; fi
niri msg action focus-window --id "$window_id"
sleep 1
# Niri 的窗口坐标相对于屏幕；wf-recorder 使用全局逻辑坐标。
niri msg -j windows > "$work/windows.json"
niri msg -j outputs > "$work/outputs.json"
geometry=$(python3 - "$work" "$window_id" "$output" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1])
windows = json.loads((p / 'windows.json').read_text())
w = next(w for w in windows
         if w['id'] == int(sys.argv[2]))
assert w['is_focused'], 'Files 窗口必须获得焦点'
assert sum(v['workspace_id'] == w['workspace_id'] for v in windows) == 1, '请先将 Files 移到空白工作区'
o = json.loads((p / 'outputs.json').read_text())[sys.argv[3]]['logical']
layout = w['layout']
x, y = layout['tile_pos_in_workspace_view']
dx, dy = layout['window_offset_in_tile']
width, height = layout['window_size']
assert 0 <= x and 0 <= y and x + width <= o['width'] and y + height <= o['height']
print(f"{round(o['x']+x+dx)-32},{round(o['y']+y+dy)-32} {width+64}x{height+64}")
PY
)
focus() { niri msg action focus-window --id "$window_id"; sleep 0.15; }
key() { focus; wtype -k "$1" -s 150; }
ctrl() { focus; wtype -M ctrl -s 100 -k "$1" -s 100 -m ctrl -s 150; }
alt() { focus; wtype -M alt -s 100 -k "$1" -s 100 -m alt -s 150; }
sidebar() {
  focus
  "$input_python" - <<'PYINPUT'
from evdev import UInput, ecodes as e
import time
with UInput({e.EV_KEY: [e.KEY_LEFTCTRL, e.KEY_B]}, name='gds-record-keyboard') as u:
    time.sleep(0.6)
    for code, value in [(e.KEY_LEFTCTRL, 1), (e.KEY_B, 1),
                        (e.KEY_B, 0), (e.KEY_LEFTCTRL, 0)]:
        u.write(e.EV_KEY, code, value)
        u.syn()
        time.sleep(0.1)
    time.sleep(0.3)
PYINPUT
}
record_pid=
stop_recording() {
  if [[ -n "$record_pid" ]]; then
    kill -INT "$record_pid" 2>/dev/null || true
    wait "$record_pid" || true
    record_pid=
  fi
}
trap stop_recording EXIT
"$recorder" -D --no-dmabuf -o "$output" -g "$geometry" -r 30 -f "$work/recording.mp4" > "$work/recorder.log" 2>&1 &
record_pid=$!
sleep 2
alt space; key Home; key Down; key Return; sleep 3
ctrl g; sleep 2
ctrl g; sleep 2
ctrl f; wtype -d 120 Bliss; sleep 2
ctrl a; key BackSpace; key Escape; sleep 2
alt space; key Home; ctrl p; sleep 3
sidebar; sleep 2
sidebar; sleep 2
ctrl p; sleep 2
ctrl p; sleep 2
alt space; key Right; sleep 3
ctrl p; alt Left; sleep 2
alt Right; sleep 2
alt Left; sleep 2
stop_recording
ffmpeg -hide_banner -loglevel error -y -i "$work/recording.mp4" \
  -filter_complex '[0:v]fps=20,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3' \
  -loop 0 "$work/file-browsing.gif"
mv "$work/file-browsing.gif" "$assets/file-browsing.gif"
printf 'GIF: %s\n原始录制和日志: %s\n' "$assets/file-browsing.gif" "$work"
