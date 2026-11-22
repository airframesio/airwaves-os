#!/usr/bin/env python3
"""Re-pin a release manifest (releases/<channel>.json) to a git ref.

Every compose, catalog and host_files URL is pointed at <ref> and each sha256
is recomputed from the file content at that ref (git show), so the manifest
always matches what devices will download. The compose/catalog `version` is
bumped only when the file content changed, which is what makes the updater
fetch it.

Usage:
  scripts/pin-manifest.py stable v1.0.38 --os-version 1.0.38 \
      --image-tag 1.0.38 --summary "Airwaves OS 1.0.38 - ..." [--archive]

  scripts/pin-manifest.py dev <main-sha>          # refresh dev host files only

Run from the repository root after the tag exists (git fetch --tags).
"""
import argparse
import datetime
import hashlib
import json
import re
import subprocess
import sys

REPO_RAW = "https://raw.githubusercontent.com/airframesio/airwaves-os"
URL_RE = re.compile(re.escape(REPO_RAW) + r"/[^/]+/(.+)$")


def file_at(ref, path):
    try:
        return subprocess.run(["git", "show", f"{ref}:{path}"], check=True,
                              capture_output=True).stdout
    except subprocess.CalledProcessError as e:
        sys.exit(f"{path} not found at {ref}: {e.stderr.decode().strip()}")


def repin(entry, ref):
    m = URL_RE.match(entry["url"])
    if not m:
        sys.exit(f"unexpected url: {entry['url']}")
    path = m.group(1)
    old = entry.get("sha256")
    entry["url"] = f"{REPO_RAW}/{ref}/{path}"
    entry["sha256"] = hashlib.sha256(file_at(ref, path)).hexdigest()
    return old != entry["sha256"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("channel")
    ap.add_argument("ref", help="tag (v1.0.38) or full commit sha")
    ap.add_argument("--os-version")
    ap.add_argument("--image-tag", help="manager/gateway image tag")
    ap.add_argument("--summary")
    ap.add_argument("--released", default=datetime.date.today().isoformat())
    ap.add_argument("--archive", action="store_true",
                    help="also write releases/<os-version>.json")
    a = ap.parse_args()

    path = f"releases/{a.channel}.json"
    with open(path) as f:
        m = json.load(f)

    comps = m["components"]
    for name in ("compose", "catalog"):
        if repin(comps[name], a.ref):
            comps[name]["version"] = str(int(comps[name]["version"]) + 1)
            print(f"{name}: content changed, version -> {comps[name]['version']}")
    changed = [h["dest"] for h in m["host_files"] if repin(h, a.ref)]
    for dest in changed:
        print(f"host file changed: {dest}")

    if a.os_version:
        m["os_version"] = a.os_version
        m["released"] = a.released
        for name in ("manager", "gateway"):
            comps[name]["version"] = a.os_version
        comps["gateway"]["control_app_version"] = a.os_version
        if a.ref.startswith("v"):
            m["notes_url"] = f"https://github.com/airframesio/airwaves-os/releases/tag/{a.ref}"
    if a.image_tag:
        for name in ("manager", "gateway"):
            comps[name]["tag"] = a.image_tag
    if a.summary:
        m["summary"] = a.summary

    out = json.dumps(m, indent=2) + "\n"
    with open(path, "w") as f:
        f.write(out)
    print(f"wrote {path}")
    if a.archive:
        if not a.os_version:
            sys.exit("--archive needs --os-version")
        with open(f"releases/{a.os_version}.json", "w") as f:
            f.write(out)
        print(f"wrote releases/{a.os_version}.json")


if __name__ == "__main__":
    main()
