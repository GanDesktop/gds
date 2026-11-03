#!/usr/bin/env python3
"""Compatibility entry point for the shared native Linux package staging."""
from pathlib import Path
import runpy

if __name__ == "__main__":
    runpy.run_path(str(Path(__file__).resolve().parents[1] / "linux/install.py"),
                   run_name="__main__")
