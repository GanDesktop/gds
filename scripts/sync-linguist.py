#!/usr/bin/env python3
"""Generate compact static Linguist lookup tables for GDS Files.

Reads GitHub Linguist's `languages.yml` (MIT license) and writes sorted Rust
arrays to `bin/files/src/entities/file/api/directory/content/linguist/generated.rs`.
"""

from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path
import urllib.request

DEFAULT_COMMIT = "cfd7a0113aa1efab99faeff38756f0ed4d48add8"
DEFAULT_OUTPUT = Path(
    "bin/files/src/entities/file/api/directory/content/linguist/generated.rs"
)

# Markup languages treated as source code (templates, stylesheets, UI SFCs, etc.)
# rather than prose documents.
CODE_MARKUP = {
    "Antlers",
    "Astro",
    "BibTeX",
    "Blade",
    "Blueprint",
    "CSS",
    "Closure Templates",
    "D2",
    "EJS",
    "Edge",
    "Go Template",
    "HTML",
    "HTML+ECR",
    "HTML+EEX",
    "HTML+ERB",
    "HTML+PHP",
    "HTML+Razor",
    "Haml",
    "Handlebars",
    "Jinja",
    "Kit",
    "LaTeX",
    "Latte",
    "Less",
    "Liquid",
    "MDX",
    "Marko",
    "Mask",
    "Mermaid",
    "Mustache",
    "Nunjucks",
    "Pug",
    "SCSS",
    "Sass",
    "Slim",
    "Svelte",
    "TeX",
    "Twig",
    "Typst",
    "Vue",
    "XSLT",
}


def strip_quotes(value: str) -> str:
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
        return value[1:-1]
    return value


def parse_languages_yml(text: str) -> dict[str, dict[str, object]]:
    """Parse the subset of YAML used by GitHub Linguist's languages.yml."""
    languages: dict[str, dict[str, object]] = {}
    current: dict[str, object] | None = None
    current_list: list[str] | None = None

    for raw_line in text.splitlines():
        if not raw_line or raw_line.lstrip().startswith("#"):
            continue
        if not raw_line.startswith(" "):
            name = strip_quotes(raw_line.rstrip(":"))
            current = {}
            languages[name] = current
            current_list = None
        elif raw_line.startswith("  - ") or raw_line.startswith("    - "):
            if current_list is not None:
                item = strip_quotes(raw_line.split("- ", 1)[1].strip())
                current_list.append(item)
        elif raw_line.startswith("  ") and not raw_line.startswith("    "):
            if current is None:
                continue
            key, _, val = raw_line.strip().partition(":")
            val = val.strip()
            if not val:
                items: list[str] = []
                current[key] = items
                current_list = items
            else:
                current[key] = strip_quotes(val)
                current_list = None

    return languages


def rust_str(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def generate_rust(languages: dict[str, dict[str, object]], commit: str) -> str:
    ext_hits: dict[str, list[tuple[str, str, bool, bool]]] = defaultdict(list)
    fn_hits: dict[str, list[tuple[str, str, bool]]] = defaultdict(list)
    code_mimes: set[str] = set()
    candidate_data_mimes: set[str] = set()
    explicit_data_mimes: set[str] = {
        "application/jrd+json",
        "application/json",
        "application/json-patch+json",
        "application/json5",
        "application/ld+json",
        "application/schema+json",
        "application/toml",
        "application/xml",
        "application/yaml",
        "text/csv",
        "text/tab-separated-values",
        "text/xml",
    }

    for name, info in languages.items():
        lang_type = str(info.get("type", ""))
        is_code = lang_type == "programming" or (
            lang_type == "markup" and name in CODE_MARKUP
        )
        mime = info.get("codemirror_mime_type")
        if isinstance(mime, str) and "/" in mime:
            mime_lower = mime.lower()
            if is_code:
                code_mimes.add(mime_lower)
            elif lang_type == "data":
                candidate_data_mimes.add(mime_lower)

        extensions = info.get("extensions")
        if isinstance(extensions, list):
            for idx, raw_ext in enumerate(extensions):
                if not isinstance(raw_ext, str) or not raw_ext.startswith("."):
                    continue
                ext = raw_ext[1:].lower()
                if ext:
                    ext_hits[ext].append((name, lang_type, is_code, idx == 0))

        filenames = info.get("filenames")
        if isinstance(filenames, list):
            for raw_fn in filenames:
                if isinstance(raw_fn, str) and raw_fn:
                    fn_hits[raw_fn].append((name, lang_type, is_code))

    code_extensions: list[tuple[str, str | None, bool]] = []
    data_extensions: list[str] = []
    xml_based_code_langs: set[str] = {"HTML"}
    for name, info in languages.items():
        if info.get("ace_mode") == "xml" or info.get("codemirror_mode") == "xml":
            xml_based_code_langs.add(name)

    for ext in sorted(ext_hits):
        hits = ext_hits[ext]
        has_prose_or_data_primary = any(
            lang_type in ("prose", "data") and is_primary
            for _, lang_type, _, is_primary in hits
        )
        if any(lang_type == "data" and is_primary for _, lang_type, _, is_primary in hits):
            data_extensions.append(ext)

        has_code = any(is_code for _, _, is_code, _ in hits)
        if not has_code or has_prose_or_data_primary:
            continue

        code_names = sorted({name for name, _, is_code, _ in hits if is_code})
        primary_code = sorted(
            {name for name, _, is_code, is_primary in hits if is_code and is_primary}
        )

        resolved_name: str | None = None
        if len(code_names) == 1:
            resolved_name = code_names[0]
        elif len(primary_code) == 1:
            resolved_name = primary_code[0]

        is_xml_based = resolved_name in xml_based_code_langs if resolved_name else False
        code_extensions.append((ext, resolved_name, is_xml_based))

    code_filenames: list[tuple[str, str | None]] = []
    data_filenames: list[tuple[str, str]] = []
    for filename in sorted(fn_hits):
        hits = fn_hits[filename]
        code_names = sorted({name for name, _, is_code in hits if is_code})
        data_names = sorted({name for name, lang_type, _ in hits if lang_type == "data"})
        has_prose_or_data = any(
            lang_type in ("prose", "data") for _, lang_type, _ in hits
        )
        if code_names and not has_prose_or_data:
            c0_name = code_names[0] if len(code_names) == 1 else None
            code_filenames.append((filename, c0_name))
        elif not code_names and len(data_names) == 1 and len(hits) == 1:
            data_filenames.append((filename, data_names[0]))

    lines = [
        "// @generated by scripts/sync-linguist.py from GitHub Linguist languages.yml",
        f"// Upstream commit: {commit}",
        "// Source: https://github.com/github-linguist/linguist/blob/main/lib/linguist/languages.yml",
        "// License: MIT (GitHub Linguist)",
        "",
        "/// Lowercase extensions (without leading dot) classified as source code:",
        "/// `(ext, unambiguous_language, is_xml_based_language)`.",
        "pub(super) const CODE_EXTENSIONS: &[(&str, Option<&str>, bool)] = &[",
    ]
    for ext, name, is_xml_based in code_extensions:
        opt = f"Some({rust_str(name)})" if name is not None else "None"
        xml_flag = "true" if is_xml_based else "false"
        lines.append(f"    ({rust_str(ext)}, {opt}, {xml_flag}),")
    lines.append("];")
    lines.append("")
    lines.append(
        "/// Exact filenames classified as source code, sorted for binary search."
    )
    lines.append("pub(super) const CODE_FILENAMES: &[(&str, Option<&str>)] = &[")
    for filename, name in code_filenames:
        opt = f"Some({rust_str(name)})" if name is not None else "None"
        lines.append(f"    ({rust_str(filename)}, {opt}),")
    lines.append("];")
    lines.append("")
    lines.append(
        "/// Exact filenames unambiguously classified as structured text data (e.g. `go.mod`)."
    )
    lines.append("pub(super) const DATA_FILENAMES: &[(&str, &str)] = &[")
    for filename, name in data_filenames:
        lines.append(f"    ({rust_str(filename)}, {rust_str(name)}),")
    lines.append("];")
    lines.append("")
    lines.append(
        "/// Lowercase extensions whose primary Linguist classification is `data` (kept in Text, not Code)."
    )
    lines.append("pub(super) const DATA_EXTENSIONS: &[&str] = &[")
    for ext in sorted(data_extensions):
        lines.append(f"    {rust_str(ext)},")
    lines.append("];")
    lines.append("")
    lines.append(
        "/// Lowercase MIME strings classified as structured data (kept in Text, not Code)."
    )
    data_mimes = explicit_data_mimes | (candidate_data_mimes - code_mimes)
    final_code_mimes = sorted(
        m
        for m in code_mimes
        if m not in data_mimes and m not in ("text/plain", "application/octet-stream")
    )
    lines.append(
        "/// Lowercase MIME strings classified as source code (for extensionless magic hits)."
    )
    lines.append("pub(super) const CODE_MIMES: &[&str] = &[")
    for mime in final_code_mimes:
        lines.append(f"    {rust_str(mime)},")
    lines.append("];")
    lines.append("")
    lines.append(
        "/// Lowercase MIME strings classified as structured data (kept in Text, not Code)."
    )
    lines.append("pub(super) const DATA_MIMES: &[&str] = &[")
    for mime in sorted(data_mimes):
        lines.append(f"    {rust_str(mime)},")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--input",
        type=Path,
        help="Path to a local languages.yml (fetches from GitHub when omitted).",
    )
    parser.add_argument(
        "--commit",
        default=DEFAULT_COMMIT,
        help="GitHub Linguist commit SHA to fetch or record in the generated header.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="Destination Rust source file.",
    )
    args = parser.parse_args()

    if args.input is not None:
        raw_text = args.input.read_text(encoding="utf-8")
    else:
        url = f"https://raw.githubusercontent.com/github-linguist/linguist/{args.commit}/lib/linguist/languages.yml"
        with urllib.request.urlopen(url, timeout=30) as response:
            raw_text = response.read().decode("utf-8")

    languages = parse_languages_yml(raw_text)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(generate_rust(languages, args.commit), encoding="utf-8")


if __name__ == "__main__":
    main()
