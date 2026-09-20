"""OneAsr app icon — 3 thick bars on teal squircle (taskbar-safe).

Run: python scripts/gen-app-icon.py
"""

from __future__ import annotations

import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parents[1] / "assets" / "icons"
OUT.mkdir(parents=True, exist_ok=True)

TEAL = (15, 118, 110, 255)  # #0f766e
WHITE = (255, 255, 255, 255)
SIZES = [16, 24, 32, 48, 64, 128, 256]


def draw_logo(size: int) -> Image.Image:
    """Three thick rounded bars on a solid teal rounded square."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    corner = max(2, round(size * 0.22))
    d.rounded_rectangle((0, 0, size - 1, size - 1), radius=corner, fill=TEAL)

    inset = size * 0.22
    area_l, area_t = inset, inset
    area_r, area_b = size - inset, size - inset
    area_w = area_r - area_l
    area_h = area_b - area_t

    n = 3
    heights = (0.48, 1.0, 0.62)
    bar_w = area_w * 0.22
    gap = (area_w - n * bar_w) / (n - 1)
    mid_y = (area_t + area_b) / 2

    for i, h_ratio in enumerate(heights):
        bh = area_h * h_ratio
        if size <= 24:
            bh = max(bh, size * 0.28)
        x0 = area_l + i * (bar_w + gap)
        y0 = mid_y - bh / 2
        y1 = mid_y + bh / 2
        d.rounded_rectangle(
            (x0, y0, x0 + bar_w, y1),
            radius=bar_w / 2,
            fill=WHITE,
        )

    return img


def png_bytes(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def write_ico(path: Path, sizes: list[int]) -> None:
    """ICO container with PNG-compressed images (Windows Vista+)."""
    pngs = [(s, png_bytes(draw_logo(s))) for s in sizes]
    count = len(pngs)
    offset = 6 + 16 * count
    entries: list[tuple[int, int, int, int]] = []
    blobs: list[bytes] = []
    for s, data in pngs:
        w = 0 if s >= 256 else s
        h = 0 if s >= 256 else s
        entries.append((w, h, len(data), offset))
        blobs.append(data)
        offset += len(data)

    out = bytearray()
    out += struct.pack("<HHH", 0, 1, count)
    for w, h, size, off in entries:
        out += struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, size, off)
    for blob in blobs:
        out += blob
    path.write_bytes(out)


def main() -> None:
    write_ico(OUT / "app-icon.ico", SIZES)
    draw_logo(512).save(OUT / "app-icon.png", "PNG")
    draw_logo(256).save(OUT / "app-icon-256.png", "PNG")
    draw_logo(32).save(OUT / "app-icon-32.png", "PNG")
    print(f"wrote {OUT / 'app-icon.ico'} ({(OUT / 'app-icon.ico').stat().st_size} bytes)")


if __name__ == "__main__":
    main()
