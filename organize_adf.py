#!/usr/bin/env python3
"""Organize Amiga .adf disk images into a front-end-ready demo folder.

Given the .adf files of a demo (named in scene convention
``TITLE (GROUP) [flags].adf``), this parses TITLE and GROUP from the first file,
creates a ``Group - Title`` directory, copies the disks into it as
``disk1.adf``, ``disk2.adf`` ... in order, and writes a ``demo.m3u`` playlist.

Usage:
    organize_adf.py "disc1.adf" "disc2.adf" ...
"""

import argparse
import re
import shutil
import sys
from pathlib import Path

# "TITLE (GROUP) ..." -> title before the first '(', group inside the first (...)
NAME_RE = re.compile(r"^(?P<title>.*?)\s*\((?P<group>[^)]*)\)")
# Optional multi-disk marker, e.g. "disc #4"
DISC_RE = re.compile(r"disc\s*#(\d+)", re.IGNORECASE)


def parse_name(path: Path):
    """Return (title, group) parsed from a filename stem; group is '' if absent."""
    stem = path.stem
    m = NAME_RE.match(stem)
    if not m:
        return stem.strip(), ""
    return m.group("title").strip(), m.group("group").strip()


def disc_number(path: Path):
    """Return the 'disc #N' number for a file, or None if not present."""
    m = DISC_RE.search(path.name)
    return int(m.group(1)) if m else None


def order_files(files):
    """Order files by 'disc #N' marker when present, else keep argument order."""
    if all(disc_number(f) is not None for f in files):
        return sorted(files, key=disc_number)
    return list(files)


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Organize .adf disk images into a 'Group - Title' demo folder."
    )
    parser.add_argument("files", nargs="+", type=Path, help="The .adf files to organize")
    args = parser.parse_args(argv)

    # Validate inputs up front.
    errors = []
    for f in args.files:
        if not f.is_file():
            errors.append(f"not a file: {f}")
        elif f.suffix.lower() != ".adf":
            errors.append(f"not a .adf file: {f}")
    if errors:
        for e in errors:
            print(f"error: {e}", file=sys.stderr)
        return 1

    title, group = parse_name(args.files[0])
    if not group:
        print(
            f"warning: no '(GROUP)' found in {args.files[0].name!r}; "
            f"using title only",
            file=sys.stderr,
        )

    dir_name = f"{group} - {title}" if group else title
    dir_name = dir_name.replace("/", "_")  # keep it a single path component
    out_dir = Path.cwd() / dir_name

    if out_dir.exists():
        print(f"warning: {dir_name!r} already exists; updating its contents",
              file=sys.stderr)
    out_dir.mkdir(exist_ok=True)

    ordered = order_files(args.files)
    disk_names = []
    for i, src in enumerate(ordered, start=1):
        dest_name = f"disk{i}.adf"
        shutil.copy2(src, out_dir / dest_name)
        disk_names.append(dest_name)

    # Write the playlist (omit year/puae_model, which aren't derivable here).
    info = f'#EXTINF:-1 title="{title}"'
    if group:
        info += f' group="{group}"'
    m3u_lines = ["#EXTM3U", info, *disk_names]
    (out_dir / "demo.m3u").write_text("\n".join(m3u_lines) + "\n")

    print(f"Created {out_dir}/ with {len(disk_names)} disk(s) and demo.m3u")
    return 0


if __name__ == "__main__":
    sys.exit(main())
