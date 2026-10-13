#!/usr/bin/env python3
"""Render notices for the normal-dependency closure of both release binaries."""

import argparse
import json
from collections import defaultdict
from pathlib import Path
from subprocess import check_output

PLATFORMS = {
    "linux": "x86_64-unknown-linux-gnu",
    "macos": "aarch64-apple-darwin",
}
SPECIAL_FILES = {
    "ring": [
        "LICENSE",
        "LICENSE-other-bits",
        "LICENSE-BoringSSL",
        "src/polyfill/once_cell/LICENSE-MIT",
        "third_party/fiat/LICENSE",
    ],
    "zstd-sys": ["LICENSE", "LICENSE.BSD-3-Clause"],
    "rustix": ["COPYRIGHT", "LICENSE-MIT"],
    "linux-raw-sys": ["COPYRIGHT", "LICENSE-MIT"],
}


def binary_packages():
    packages = {}
    platforms = defaultdict(set)
    for label, target in PLATFORMS.items():
        data = json.loads(
            check_output(
                [
                    "cargo",
                    "metadata",
                    "--locked",
                    "--format-version",
                    "1",
                    "--filter-platform",
                    target,
                ]
            )
        )
        by_id = {package["id"]: package for package in data["packages"]}
        nodes = {node["id"]: node for node in data["resolve"]["nodes"]}
        pending = [data["resolve"]["root"]]
        seen = set()
        while pending:
            package_id = pending.pop()
            if package_id in seen:
                continue
            seen.add(package_id)
            package = by_id[package_id]
            if any("proc-macro" in item["kind"] for item in package["targets"]):
                continue
            if package["name"] != "herdr-lineup":
                key = (package["name"], package["version"])
                packages[key] = package
                platforms[key].add(label)
            for dependency in nodes[package_id]["deps"]:
                if any(kind["kind"] in (None, "normal") for kind in dependency["dep_kinds"]):
                    pending.append(dependency["pkg"])
    return packages, platforms


def selected_files(package):
    if package["name"] in SPECIAL_FILES:
        return SPECIAL_FILES[package["name"]]
    source = Path(package["manifest_path"]).parent
    if "MIT" in package["license"]:
        for filename in ("LICENSE-MIT", "LICENSE-MIT.txt"):
            if (source / filename).is_file():
                return [filename]
    for filename in ("LICENSE", "LICENSE.txt"):
        if (source / filename).is_file():
            return [filename]
    raise RuntimeError(f"No license text for {package['name']}@{package['version']}")


def selected_terms(declared):
    if declared == "Apache-2.0 AND ISC":
        return declared
    if "MIT" in declared:
        return "MIT"
    return declared


def render():
    packages, platforms = binary_packages()
    output = [
        "# Third-party Rust binary dependency licenses\n\n",
        "This file covers the normal-dependency closure of the Linux x86_64 and macOS\n",
        "arm64 release binaries in `Cargo.lock`. Build dependencies, development\n",
        "dependencies, procedural macros, and their dependencies are excluded. The\n",
        "macOS-only dependency is marked below. For alternative licenses, the selected\n",
        "terms are shown. License text is reproduced once where pinned crates contain\n",
        "byte-identical text. The `ring` entries include BoringSSL, embedded\n",
        "once_cell, and fiat-crypto terms; `zstd-sys` includes its Rust-binding\n",
        "and vendored C terms.\n\n",
        "<!-- dependency-list:start -->\n",
        "| Crate | Declared SPDX expression | Distributed terms | Platforms |\n",
        "| --- | --- | --- | --- |\n",
    ]
    for key in sorted(packages):
        package = packages[key]
        output.append(
            f"| `{key[0]}@{key[1]}` | `{package['license']}` | "
            f"`{selected_terms(package['license'])}` | "
            f"{', '.join(sorted(platforms[key]))} |\n"
        )
    output.append("<!-- dependency-list:end -->\n")

    texts = defaultdict(list)
    for key in sorted(packages):
        package = packages[key]
        source = Path(package["manifest_path"]).parent
        for filename in selected_files(package):
            content = (source / filename).read_text(encoding="utf-8").rstrip()
            if package["name"] == "chrono" and filename == "LICENSE.txt":
                content = content.split("[2]:", 1)[0].rstrip()
            texts[content].append(f"{key[0]}@{key[1]} `{filename}`")

    for index, (content, labels) in enumerate(
        sorted(texts.items(), key=lambda item: item[1][0]), 1
    ):
        output.append(f"\n## License text {index}\n\nSource crates and license files:\n\n")
        for label in labels:
            output.append(f"- {label}\n")
        output.append(f"\n````text\n{content}\n````\n")
    return "".join(output)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--output", default="THIRD-PARTY-LICENSES.md")
    args = parser.parse_args()
    output = Path(args.output)
    rendered = render()
    if args.check:
        if output.read_text(encoding="utf-8") != rendered:
            raise SystemExit(f"{output} is out of date")
        print(f"{output} matches pinned crate license texts")
    else:
        output.write_text(rendered, encoding="utf-8", newline="\n")
        print(f"wrote {output}")


if __name__ == "__main__":
    main()
