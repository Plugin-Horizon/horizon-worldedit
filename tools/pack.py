#!/usr/bin/env python3
"""Pack this bundle into the .hnmod the launcher installs.

The archive holds `horizon-native.json` and only the `.rs` and `.toml` files of
`mixins/<id>/`: `horizon_mods::index::native::Bundle::read` rejects anything
else, and checks the manifest against the bundle's `horizon-mixin.toml` (the
same id, version and `compatibility.horizon_revision`).

    python3 tools/pack.py            # worldedit-native-<version>.hnmod here
    python3 tools/pack.py --out DIR  # ...into DIR
    python3 tools/pack.py --check    # validate only, write nothing
"""
import argparse
import json
import pathlib
import re
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "horizon-native.json"


def read_manifest():
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    required = ("schema", "id", "version", "game", "horizon", "mixins")
    missing = [key for key in required if not manifest.get(key)]
    if missing:
        sys.exit(f"horizon-native.json is missing {', '.join(missing)}")
    if manifest["schema"] != 1:
        sys.exit(f"schema must be 1, not {manifest['schema']}")
    for key in ("horizon", "mixins"):
        if not re.fullmatch(r"[0-9a-f]{40}", manifest[key]):
            sys.exit(f"{key} must be a 40-character commit, not {manifest[key]!r}")
    return manifest


def read_mixin(path):
    text = path.read_text(encoding="utf-8")
    def field(section, key):
        block = re.search(rf"^\[{section}\]$(.*?)(?=^\[|\Z)", text, re.S | re.M)
        found = block and re.search(rf'^{key}\s*=\s*"([^"]*)"', block.group(1), re.M)
        return found.group(1) if found else None
    return {
        "id": field("mod", "id"),
        "version": field("mod", "version"),
        "horizon": field("compatibility", "horizon_revision"),
    }


def files(manifest):
    folder = ROOT / "mixins" / manifest["id"]
    mixin = folder / "horizon-mixin.toml"
    if not mixin.is_file():
        sys.exit(f"missing {mixin.relative_to(ROOT)}")
    actual = read_mixin(mixin)
    for key in ("id", "version"):
        if actual[key] != manifest[key]:
            sys.exit(f"horizon-mixin.toml's {key} is {actual[key]!r}, the manifest says {manifest[key]!r}")
    if actual["horizon"] != manifest["horizon"]:
        sys.exit(
            "horizon-mixin.toml's compatibility.horizon_revision is "
            f"{actual['horizon']!r}, the manifest says {manifest['horizon']!r}"
        )
    yield "horizon-native.json", MANIFEST
    for path in sorted(folder.iterdir()):
        if path.suffix in (".rs", ".toml"):
            yield f"mixins/{manifest['id']}/{path.name}", path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=pathlib.Path, default=ROOT)
    parser.add_argument("--check", action="store_true", help="validate, write nothing")
    arguments = parser.parse_args()
    manifest = read_manifest()
    entries = list(files(manifest))
    if arguments.check:
        print(f"ok: {manifest['id']} {manifest['version']}, {len(entries) - 1} source files")
        return
    arguments.out.mkdir(parents=True, exist_ok=True)
    out = arguments.out / f"worldedit-native-{manifest['version']}.hnmod"
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, path in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, path.read_bytes())
    print(f"{out} ({out.stat().st_size} bytes, {len(entries)} files)")


if __name__ == "__main__":
    main()
