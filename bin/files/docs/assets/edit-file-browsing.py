#!/usr/bin/env python3
"""将本次约 50 秒的原始录制剪成约 20 秒，所有保留片段均为原速。

用法：python3 edit-file-browsing.py recording.mp4 [output.gif]
时间点对应 2026-11-03 空白工作区居中录制的版本；重新录制后须重新核对时间点。
只删除静止等待和末尾重复导航，不加速动画。输出 960 像素宽、20 fps、无限循环。
"""
import argparse
import subprocess
from pathlib import Path

# 原始录像时间（秒）；保留每个操作前的短暂静止和完成后的阅读时间。
SEGMENTS = [
    (0.0, 0.7),      # Workspace 开场
    (3.1, 4.3),      # 进入 Images
    (6.5, 7.5),      # 切换列表
    (9.05, 10.05),   # 恢复网格
    (11.5, 13.3),    # 输入 Bliss 并展示筛选结果
    (15.1, 16.3),    # 清空筛选
    (18.7, 20.4),    # 展开图片详情
    (24.6, 26.8),    # 折叠侧边栏（完整动画）
    (30.4, 32.4),    # 展开侧边栏（完整动画）
    (33.0, 34.6),    # 折叠详情页（完整动画）
    (35.9, 37.2),    # 展开详情页（完整动画）
    (38.8, 40.7),    # 选择下一张图片、更新详情
    (41.4, 43.8),    # 关闭详情并返回 Workspace，衔接循环
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path, nargs='?',
                        default=Path(__file__).with_name('file-browsing.gif'))
    args = parser.parse_args()
    count = len(SEGMENTS)
    graph = [f'[0:v]scale=960:-1:flags=lanczos,split={count}'
             + ''.join(f'[s{i}]' for i in range(count))]
    for i, (start, end) in enumerate(SEGMENTS):
        graph.append(f'[s{i}]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{i}]')
    graph.append(''.join(f'[v{i}]' for i in range(count))
                 + f'concat=n={count}:v=1:a=0,fps=20,split[a][b]')
    graph.extend(['[a]palettegen=stats_mode=diff[p]',
                  '[b][p]paletteuse=dither=bayer:bayer_scale=3[out]'])
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y',
                    '-i', str(args.source), '-filter_complex', ';'.join(graph),
                    '-map', '[out]', '-loop', '0', str(args.output)], check=True)
    print(f'{args.output}: {sum(b-a for a, b in SEGMENTS):.2f} 秒，原速')


if __name__ == '__main__':
    main()
