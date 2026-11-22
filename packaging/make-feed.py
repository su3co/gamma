#!/usr/bin/env python3
"""Write an update feed (Tauri updater format) for one release.

    python3 packaging/make-feed.py --version 0.1.1 --channel stable \
        --notes CHANGELOG.md --base https://example.org/gamma \
        --artifact darwin-aarch64=target/release/bundle/macos/gamma.app.tar.gz \
        --artifact linux-x86_64=dist/gamma_0.1.1_amd64.AppImage \
        --out feed/

writes feed/<channel>/latest.json and copies each artifact next to it, so the
folder can be served as is (any static file host). Each artifact needs its
minisign signature (<artifact>.sig, made by `tauri build` with
TAURI_SIGNING_PRIVATE_KEY); the app refuses an update whose signature does
not verify with the public key compiled into it.
"""

import argparse
import datetime as dt
import json
import re
import shutil
from pathlib import Path


def notes_for(changelog: Path, version: str) -> str:
    if not changelog or not changelog.is_file():
        return ""
    text = changelog.read_text()
    m = re.search(rf"^## \[?{re.escape(version)}\]?.*?$(.*?)(?=^## |\Z)", text, re.M | re.S)
    return m.group(1).strip() if m else ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True)
    ap.add_argument("--channel", default="stable", choices=["stable", "beta"])
    ap.add_argument("--notes", type=Path, help="CHANGELOG.md (the section of this version is used)")
    ap.add_argument("--base", required=True, help="URL the feed folder is served from")
    ap.add_argument("--artifact", action="append", default=[], help="PLATFORM=PATH (darwin-aarch64, linux-x86_64, linux-aarch64, windows-x86_64)")
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args()
    ch = a.out / a.channel
    ch.mkdir(parents=True, exist_ok=True)
    platforms = {}
    for spec in a.artifact:
        plat, path = spec.split("=", 1)
        p = Path(path)
        sig = Path(str(p) + ".sig")
        if not sig.is_file():
            raise SystemExit(f"{p}: no signature {sig.name}; build with TAURI_SIGNING_PRIVATE_KEY set")
        name = f"{a.version}/{plat}/{p.name}"
        (ch / a.version / plat).mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, ch / name)
        platforms[plat] = {"signature": sig.read_text().strip(), "url": f"{a.base.rstrip('/')}/{a.channel}/{name}"}
    feed = {
        "version": a.version,
        "notes": notes_for(a.notes, a.version) if a.notes else "",
        "pub_date": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    (ch / "latest.json").write_text(json.dumps(feed, indent=2))
    print(f"wrote {ch / 'latest.json'} with {', '.join(platforms) or 'no platforms'}")


if __name__ == "__main__":
    main()
