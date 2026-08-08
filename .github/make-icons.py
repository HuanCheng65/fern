#!/usr/bin/env python3
"""从走线定义生成应用图标与 favicon。

标志的全部几何是 7×9 网格上的八段走线（见 docs/fern-brand-system.html），
所以图标是**算出来的**，不是画出来再导出的。改一格就是改这里的一个数字，
所有尺寸跟着重算——手工导出的一堆 PNG 迟早会和规范对不上。

不走「栅格化 SVG」那条路：标志是一格一格的方块，每一档尺寸都把格子边长取整
再居中，才能在 32px 上不糊。这正是规范里「16px 场景用每格恰为 2px 的精确版」
那条要求的推广。

    python3 .github/make-icons.py
"""

from __future__ import annotations

import hashlib
import json
import zlib
from pathlib import Path

from PIL import Image, ImageDraw

# 色彩系统只有四个值，图标用其中两个：墨松底，嫩芽标志。
PINE = (0x0E, 0x20, 0x18, 0xFF)
SPROUT = (0xBF, 0xE4, 0xB2, 0xFF)

# 走线：茎底 → 右侧上行 → 顶部左行 → 左侧下行 → 底部右行 → 收入内圈 → 芽尖。
# 名字跟着坐标走，别拆成两个列表——它们会漂。
RUNS = [
    (7, 9, 7, 8, "茎"),
    (7, 7, 7, 1, "右侧上行"),
    (6, 1, 1, 1, "顶部左行"),
    (1, 2, 1, 7, "左侧下行"),
    (2, 7, 5, 7, "底部右行"),
    (5, 6, 5, 3, "内圈右侧"),
    (4, 3, 3, 3, "内圈顶部"),
    (3, 4, 3, 5, "内圈左侧 · 芽尖"),
]
# 16px 精确版：茎缩短一格，7×8 格，每格恰为 2px。
RUNS_COMPACT = [(7, 8, 7, 8, "茎")] + RUNS[1:]

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "fern-ui/src-tauri/icons"
PUBLIC = ROOT / "fern-ui/public"
# macOS 26 的图标在 Icon Composer 里合成，但喂给它的标志还是这里算出来的。
MACOS_ASSET = ICONS / "macos/Fern.icon/Assets/fern-mark.svg"

# 保护区是四周各一格，但图标底板本身已经是留白，取 14% 与规范的构造图一致。
PAD_RATIO = 0.14
# 应用图标圆角 22.5%。
CORNER_RATIO = 0.225


def cells(runs) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    seen = set()
    for ax, ay, bx, by, _ in runs:
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


def mark_svg() -> str:
    """Icon Composer 的输入资产。一段走线一个 `rect`，不是一格一个。"""
    grid = cells(RUNS)
    x0 = min(c[0] for c in grid)
    y0 = min(c[1] for c in grid)
    width = max(c[0] for c in grid) - x0 + 1
    height = max(c[1] for c in grid) - y0 + 1
    rects = "".join(
        f'\n  <rect x="{min(ax, bx) - x0}" y="{min(ay, by) - y0}" '
        f'width="{abs(bx - ax) + 1}" height="{abs(by - ay) + 1}"/><!-- {label} -->'
        for ax, ay, bx, by, label in RUNS
    )
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" '
        f'viewBox="0 0 {width} {height}" width="{width * 20}" height="{height * 20}" '
        'shape-rendering="crispEdges" fill="currentColor">\n'
        "  <!-- Fern mark · 直角螺线 2.5 圈 · 圈距 1 格 · 茎 2 格 -->"
        f"{rects}\n</svg>\n"
    )


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


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    PUBLIC.mkdir(parents=True, exist_ok=True)
    MACOS_ASSET.parent.mkdir(parents=True, exist_ok=True)

    written = []
    for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
        tile(size).save(ICONS / name)
        written.append(ICONS / name)

    # Windows 的 ico 要多档，系统按上下文自己挑。
    tile(256).save(
        ICONS / "icon.ico",
        sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    written.append(ICONS / "icon.ico")

    # macOS 的 .icns 和 Assets.car 不在这里生成，所以要能看出它们什么时候过期了。
    before = MACOS_ASSET.read_text(encoding="utf-8") if MACOS_ASSET.exists() else None
    MACOS_ASSET.write_text(mark_svg(), encoding="utf-8")
    written.append(MACOS_ASSET)

    (PUBLIC / "favicon.svg").write_text(favicon_svg(), encoding="utf-8")
    written.append(PUBLIC / "favicon.svg")

    # 手工维护的清单会漂——已经漂过一次（`icon.icns` 删了，清单里还记着）。
    (ICONS / ".generated.json").write_text(
        json.dumps(
            {
                str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sorted(written, key=lambda path: str(path))
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )

    for path in written:
        print(f"写入 {path.relative_to(ROOT)}")
    if before is not None and before != MACOS_ASSET.read_text(encoding="utf-8"):
        composer = MACOS_ASSET.parent.parent.relative_to(ROOT)
        print()
        print("走线变了，但 macOS 的图标这个脚本生不出来——Assets.car 和 Fern.icns")
        print(f"是 Icon Composer 的产物。到 macOS 上打开 {composer} 重新导出，")
        print("否则 Linux 和 Windows 换了新标志，macOS 还是旧的，而且不会报错。")
    assert zlib  # 保持导入，PNG 编码走它


if __name__ == "__main__":
    main()
