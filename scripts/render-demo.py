#!/usr/bin/env python3
"""Render excerpts of the verified demo transcript; requires Pillow 12.3.0."""

import argparse
from pathlib import Path
import textwrap

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
BG = "#0b1220"
TEXT = "#dbe4ef"
MUTED = "#94a3b8"
GREEN = "#6ee7b7"
RED = "#fda4af"
BLUE = "#7dd3fc"
YELLOW = "#fcd34d"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--font", type=Path, help="Path to a monospace TTF or TTC font")
    args = parser.parse_args()
    candidates = [args.font] if args.font else [
        Path("/System/Library/Fonts/Menlo.ttc"),
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        Path("C:/Windows/Fonts/consola.ttf"),
    ]
    font_path = next((p for p in candidates if p.is_file()), None)
    if font_path is None:
        parser.error("No monospace font found; pass --font /path/to/font.ttf")
    font = ImageFont.truetype(str(font_path), 18)
    small = ImageFont.truetype(str(font_path), 15)
    transcript = (ROOT / "target/demo/transcript.txt").read_text(encoding="utf-8")
    blocks = transcript.split("$ dbdiff ")[1:]
    if len(blocks) != 4:
        parser.error("Expected four captured commands; run examples/demo/run.py first")
    first = blocks[0].splitlines()
    migration_start = first.index("Generated migration")
    summary = next(line for line in first if line.startswith("Summary:"))
    diff_lines = [line for line in first[1:migration_start] if "[unchanged]" not in line]
    sql_lines = first[migration_start + 1:first.index(summary)]
    scenes = [
        ("01 / Inspect schema drift", ["$ dbdiff " + first[0], "", *diff_lines, summary]),
        ("02 / Review generated SQL", ["$ dbdiff " + first[0], "", "Generated migration", *sql_lines]),
        ("03 / Gate CI on drift", ("$ dbdiff " + blocks[1]).strip().splitlines()),
        ("04 / Reject blocking operations", ("$ dbdiff " + blocks[2]).strip().splitlines()),
        ("05 / Confirm matching schemas", ("$ dbdiff " + blocks[3]).strip().splitlines()),
    ]
    frames = []
    durations = []
    for title, lines in scenes:
        wrapped = []
        for line in lines:
            wrapped.extend(textwrap.wrap(line, width=88, subsequent_indent="    ",
                                         replace_whitespace=False, drop_whitespace=False) or [""])
        for visible in range(1, len(wrapped) + 1):
            frame = Image.new("RGB", (1100, 630), BG)
            draw = ImageDraw.Draw(frame)
            draw.rounded_rectangle((12, 12, 1088, 618), radius=16, fill="#111b2c", outline="#334155")
            draw.rounded_rectangle((13, 13, 1087, 64), radius=15, fill="#1c2b40")
            draw.rectangle((13, 44, 1087, 64), fill="#1c2b40")
            for x, color in [(37, "#fb7185"), (59, "#fbbf24"), (81, "#34d399")]:
                draw.ellipse((x - 5, 33 - 5, x + 5, 33 + 5), fill=color)
            draw.text((114, 22), "dbdiff / offline demo", fill=TEXT, font=small)
            draw.text((35, 87), title, fill=BLUE, font=font)
            y = 133
            for line in wrapped[:visible]:
                stripped = line.lstrip()
                color = TEXT
                if stripped.startswith(("+", "- ADD")):
                    color = GREEN
                elif stripped.startswith(("- column", "!", "!!")):
                    color = RED
                elif stripped.startswith(("~", "exit code: 1", "exit code: 3")):
                    color = YELLOW
                elif stripped.startswith("exit code: 0"):
                    color = GREEN
                elif stripped.startswith("$"):
                    color = BLUE
                draw.text((35, y), line, fill=color, font=font)
                y += 25
            if y > 571:
                raise ValueError("Scene exceeds terminal height; shorten the excerpt")
            draw.line((35, 576, 1065, 576), fill="#334155")
            draw.text((35, 591), "Actual CLI output · selected excerpts · no database required", fill=MUTED, font=small)
            frames.append(frame)
            durations.append(3200 if visible == len(wrapped) else 80)
        if title.startswith("01"):
            frame.save(ROOT / "doc/assets/demo.png")
    frames[0].save(ROOT / "doc/assets/demo.gif", save_all=True, append_images=frames[1:],
                   duration=durations, loop=0, optimize=True, disposal=2)
    print(f"Rendered {len(frames)} frames to doc/assets/demo.gif and demo.png")


if __name__ == "__main__":
    main()
