# README 素材录制

这些素材来自 Niri / Wayland 上实际运行的 GDS Files。所有素材统一使用英文界面，沿用本机 DMS 系统主题、字体和经典壁纸；不合成应用画面。终端截图展示实际运行的 `fastfetch`。

## 准备

需要已安装的 `gds-files`、`niri`、DMS，以及 `grim`、`wf-recorder`、`ffmpeg`、`wtype`、`wl-paste`、`dbus-run-session`、`fastfetch` 和带 `evdev` 模块的 Python 3.11+。当前用户需要有 `/dev/uinput` 的访问权限，屏幕逻辑尺寸至少为 1280 × 840。

在仓库根目录运行：

```sh
python3 bin/files/docs/assets/fetch-wallpapers.py "$HOME/.cache/gds-readme-wallpapers"
python3 bin/files/docs/assets/record-demos.py --wallpapers "$HOME/.cache/gds-readme-wallpapers"
```

壁纸下载地址取自 [来源清单](theme-sync-sources.md)。缓存目录须能被桌面进程读取，建议使用 `~/.cache`（远程执行环境的 `/tmp` 可能与桌面隔离）。原图只放在指定缓存目录；演示文件夹使用缩小到 2560 × 1440 范围内的 Big Sur、Bliss 和 Mojave 副本，便于预览。

录制使用真实键盘输入，会临时切换工作区与当前屏幕的壁纸。请在桌面空闲时运行；焦点被其他窗口抢走时脚本会停止。它创建独立的演示目录、英文 Files 配置、状态和 D-Bus 会话，结束后关闭演示窗口并恢复原壁纸与焦点。已有 Files 窗口和个人设置不受影响。

## 单独录制

`--scene` 可重复指定。默认录制下表中的全部场景，包括终端。

| 场景 | 输出 | 内容 |
| --- | --- | --- |
| `overview` | `overview.png` | 图片网格、详情预览与侧边栏 |
| `browsing` | `file-browsing.gif` | 进入目录、切换视图、筛选与预览 |
| `copy-path` | `copy-path.png`、`copy-path.gif` | 右键菜单与复制后粘贴到地址栏；校验剪贴板结果 |
| `keyboard` | `keyboard-navigation.gif` | 键盘打开新页签、选择文件、预览和切换页签 |
| `theme` | `theme-sync.gif` | 原有 16 张经典壁纸及系统配色，每张停留 1 秒 |
| `appearance` | `appearance-settings.png` | 主题、透明度、圆角、动画和字体设置 |
| `opacity` | `opacity-opaque.png`、`opacity-translucent.png` | 同一位置对比不透明与半透明背景 |
| `corners` | `corners-square.png`、`corners-rounded.png` | 同一窗口左上角的直角与圆角特写 |
| `font` | `font-size.gif` | 本机字体在 14 px 与 20 px 下的实际效果 |
| `animations` | `interface-animations.gif` | 侧边栏与详情面板的展开、收起动画 |
| `picker` | `file-picker.png` | 多选图片的原生文件选择器 |
| `terminal` | `terminal.png` | 英文文件列表与内置终端中的 fastfetch |

例如：

```sh
python3 bin/files/docs/assets/record-demos.py \
  --wallpapers "$HOME/.cache/gds-readme-wallpapers" --scene overview --scene picker
```

`record-file-browsing.sh` 是 `--scene browsing` 的快捷入口，同样需要 `--wallpapers` 参数。可用 `INPUT_PYTHON` 指定带 evdev 的解释器，用 `GRIM` 和 `WF_RECORDER` 指定可执行文件。

终端场景使用临时 fastfetch 配置，展示 NixOS 标志和实际系统信息。如需试录而不覆盖 README 素材，请指定其他输出目录：

```sh
python3 bin/files/docs/assets/record-demos.py \
  --wallpapers "$HOME/.cache/gds-readme-wallpapers" --scene terminal --output /tmp/gds-terminal-preview
```

## 检查与导出

PNG 截取 1200 × 760 的窗口及四周 24 像素桌面；圆角对比截取左上角 400 × 260 的区域。动作录制保留原速，以 15 fps、960 像素宽导出 GIF；主题与字号对比由真实截图组成。原始视频、截图序列、运行日志及环境记录保存在脚本打印的临时目录。

录制后检查每个 PNG 和 GIF 的关键帧，确认缩略图加载、操作结果、文字和边缘没有被裁切。可以重新导出原始动作录像：

```sh
python3 bin/files/docs/assets/edit-file-browsing.py /path/to/recording.mp4 /path/to/output.gif
```

导出脚本不再使用某一次录像的固定剪辑时间点，也不会加速动画。
