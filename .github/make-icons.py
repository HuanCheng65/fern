#!/usr/bin/env python3
"""从走线定义生成应用图标与 favicon。

标志的全部几何是 7×9 网格上的八段走线（见 docs/fern-brand-system.html），
所以图标是**算出来的**，不是画出来再导出的。改一格就是改这里的一个数字，
所有尺寸跟着重算——手工导出的一堆 PNG 迟早会和规范对不上。

不走「栅格化 SVG」那条路：标志是一格一格的方块，每一档尺寸都把格子边长取整
再居中，才能在 32px 上不糊。这正是规范里「16px 场景用每格恰为 2px 的精确版」
那条要求的推广。

    python3 .github/make-icons.py

已经被人手工换掉的文件不会被覆盖（比如 macOS 那个走 Icon Composer 的
icns）——脚本记着自己上次写出去的样子，对不上就让开。
"""

from __future__ import annotations

import hashlib
import json
import struct
from pathlib import Path

from PIL import Image, ImageDraw

# 色彩系统只有四个值，图标用其中两个：墨松底，嫩芽标志。
PINE = (0x0E, 0x20, 0x18, 0xFF)
SPROUT = (0xBF, 0xE4, 0xB2, 0xFF)

# 走线：茎底 → 右侧上行 → 顶部左行 → 左侧下行 → 底部右行 → 收入内圈 → 芽尖。
RUNS = [
    (7, 9, 7, 8),
    (7, 7, 7, 1),
    (6, 1, 1, 1),
    (1, 2, 1, 7),
    (2, 7, 5, 7),
    (5, 6, 5, 3),
    (4, 3, 3, 3),
    (3, 4, 3, 5),
]
# 16px 精确版：茎缩短一格，7×8 格，每格恰为 2px。
RUNS_COMPACT = [(7, 8, 7, 8)] + RUNS[1:]

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "fern-ui/src-tauri/icons"
PUBLIC = ROOT / "fern-ui/public"
# 记下每个生成出来的文件长什么样，好在下次认出哪些已经被人换掉了。
MANIFEST = ICONS / ".generated.json"

# 保护区是四周各一格，但图标底板本身已经是留白，取 14% 与规范的构造图一致。
PAD_RATIO = 0.14
# 应用图标圆角 22.5%。
CORNER_RATIO = 0.225


def cells(runs) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    seen = set()
    for ax, ay, bx, by in runs:
        dx = (bx > ax) - (bx < ax)
        dy = (by > ay) - (by < ay)
        x, y = ax, ay
        while True:
            if (x, y) not in seen:
                seen.add((x, y))
                out.append((x, y))
            if (x, y) == (bx, by):
                break
            x += dx
            y += dy
    return out


def draw_mark(image: Image.Image, runs, colour, pad_ratio: float) -> None:
    """把标志画进一张正方形的图。格子边长取整，保证每一档尺寸都是实边。"""
    grid = cells(runs)
    xs = [c[0] for c in grid]
    ys = [c[1] for c in grid]
    x0, y0 = min(xs), min(ys)
    width = max(xs) - x0 + 1
    height = max(ys) - y0 + 1

    size = image.width
    cell = max(1, int(size * (1 - 2 * pad_ratio)) // max(width, height))
    ox = (size - width * cell) // 2
    oy = (size - height * cell) // 2

    pen = ImageDraw.Draw(image)
    for x, y in grid:
        left = ox + (x - x0) * cell
        top = oy + (y - y0) * cell
        pen.rectangle([left, top, left + cell - 1, top + cell - 1], fill=colour)


def tile(size: int) -> Image.Image:
    """墨松底板加圆角，再落标志。"""
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ImageDraw.Draw(image).rounded_rectangle(
        [0, 0, size - 1, size - 1], radius=int(size * CORNER_RATIO), fill=PINE
    )
    draw_mark(image, RUNS, SPROUT, PAD_RATIO)
    return image


def icns(images: dict[str, Image.Image]) -> bytes:
    """手写 icns：一个 'icns' 容器，每种尺寸一段 PNG。

    PIL 的 ICNS 写入依赖 macOS 的 iconutil，Linux 上用不了。格式本身很简单，
    自己拼比让构建机分平台更省事。
    """
    chunks = b""
    for kind, image in images.items():
        import io

        buffer = io.BytesIO()
        image.save(buffer, format="PNG")
        payload = buffer.getvalue()
        chunks += kind.encode("ascii") + struct.pack(">I", len(payload) + 8) + payload
    return b"icns" + struct.pack(">I", len(chunks) + 8) + chunks


def favicon_svg() -> str:
    """浏览器标签页用的 favicon。走精确版，`currentColor` 继承环境色。"""
    grid = cells(RUNS_COMPACT)
    xs = [c[0] for c in grid]
    ys = [c[1] for c in grid]
    x0, y0 = min(xs), min(ys)
    rects = "".join(
        f'<rect x="{x - x0}" y="{y - y0}" width="1" height="1"/>' for x, y in grid
    )
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 7 8" '
        'shape-rendering="crispEdges" fill="#35714A">'
        f"{rects}</svg>\n"
    )


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_manifest() -> dict[str, str]:
    try:
        return json.loads(MANIFEST.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def write(path: Path, payload: bytes, manifest: dict[str, str], seen: dict[str, str]) -> None:
    """写一个生成出来的文件，但**不覆盖手工换过的那一份**。

    某些图标是人做的而不是算的——比如 macOS 那个走 Icon Composer 的 icns。
    脚本记着自己上次写出去的样子，对不上就说明有人换过，让开。
    """
    key = path.name
    if path.exists() and manifest.get(key) not in (None, digest(path)):
        print(f"  跳过 {key}：已被替换为手工版本")
        seen[key] = manifest[key]
        return
    path.write_bytes(payload)
    seen[key] = digest(path)


def png_bytes(image: Image.Image, **options) -> bytes:
    import io

    buffer = io.BytesIO()
    image.save(buffer, format=options.pop("format", "PNG"), **options)
    return buffer.getvalue()


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    PUBLIC.mkdir(parents=True, exist_ok=True)
    manifest = load_manifest()
    seen: dict[str, str] = {}

    for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
        write(ICONS / name, png_bytes(tile(size)), manifest, seen)

    # Windows 的 ico 要多档，系统按上下文自己挑。
    write(
        ICONS / "icon.ico",
        png_bytes(
            tile(256),
            format="ICO",
            sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
        ),
        manifest,
        seen,
    )

    write(
        ICONS / "icon.icns",
        icns(
            {
                "icp4": tile(16),
                "icp5": tile(32),
                "icp6": tile(64),
                "ic07": tile(128),
                "ic08": tile(256),
                "ic09": tile(512),
                "ic13": tile(512),
                "ic14": tile(1024),
            }
        ),
        manifest,
        seen,
    )

    write(PUBLIC / "favicon.svg", favicon_svg().encode("utf-8"), manifest, seen)

    MANIFEST.write_text(json.dumps(seen, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"图标已写入 {ICONS.relative_to(ROOT)} 与 {PUBLIC.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
