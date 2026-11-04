#!/usr/bin/env python3
"""Export a real-time recording to GIF without recording-specific cut points."""
import argparse
from pathlib import Path
import subprocess


def export(source, output):
    subprocess.run([
        'ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', str(source),
        '-filter_complex',
        'fps=15,scale=960:-2:flags=lanczos,split[a][b];'
        '[a]palettegen=stats_mode=diff[p];'
        '[b][p]paletteuse=dither=bayer:bayer_scale=3',
        '-loop', '0', str(output),
    ], check=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path, nargs='?',
                        default=Path(__file__).with_name('file-browsing.gif'))
    args = parser.parse_args()
    export(args.source, args.output)
