#!/usr/bin/env python3
"""Download the classic wallpapers credited in theme-sync-sources.md."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import re
import subprocess


def download(item):
    title, url, destination = item
    if not destination.exists():
        temporary = destination.with_suffix(destination.suffix + '.part')
        raw = url.replace('https://github.com/', 'https://raw.githubusercontent.com/').replace('/blob/', '/')
        subprocess.run(['curl', '--fail', '--location', '--silent', '--show-error', '--retry', '3',
                        raw, '--output', str(temporary)], check=True)
        temporary.replace(destination)
    print(f'{title}: {destination}', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    sources = Path(__file__).with_name('theme-sync-sources.md').read_text()
    items = []
    for number, title, url in re.findall(r'\| (\d+) \| ([^|]+?) \| \[Wallpaper\]\(([^)]+)\)', sources):
        name = re.sub(r'[^A-Za-z0-9]+', '-', title).strip('-')
        suffix = Path(url).suffix
        items.append((title, url, args.directory / f'{number}-{name}{suffix}'))
    with ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(download, items))
