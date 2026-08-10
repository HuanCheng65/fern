#!/usr/bin/env python3
"""把字体裁到站上真正用到的字，输出 woff2 到 static/fonts/。

中文字库整套七八兆，站上只用到几百个字，所以按源码里出现的字符裁。

三件事值得说明：

- 注释要剔掉。代码里的中文注释一个字都不会显示，算进去白白多裁一倍。
- 嵌进来的 fern-ui 组件也要算。命令面板是真组件，它的中文标签会出现在页面上。
- 裁漏了不会出豆腐块——字体栈后面挂着系统中文字体，最坏是那几个字换了个字面。

改了文案之后重跑：

    python3 scripts/subset-fonts.py <字体源目录>

源目录里需要 InterVariable.ttf 和 HarmonyOS_Sans_SC_{Regular,Medium,Bold}.ttf。
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent  # fern-site/
REPO = ROOT.parent
OUT = ROOT / 'static' / 'fonts'

# 站自己的全部源码
SITE = [
    p
    for p in (ROOT / 'src').rglob('*')
    if p.is_file() and p.suffix in ('.svelte', '.js', '.ts', '.css', '.html')
]
# 站里真正引入的产品组件（改了 Direct.svelte 的 import，这里跟着改）
UI = [
    REPO / 'fern-ui/src/components/CommandPalette.svelte',
    REPO / 'fern-ui/src/components/Overlay.svelte',
    REPO / 'fern-ui/src/components/Cover.svelte',
    REPO / 'fern-ui/src/lib/palette.svelte.ts',
    REPO / 'fern-ui/src/lib/instances.svelte.ts',
]
# 文案母本：站上可能还没排进去的字也先裁进来
DOCS = [REPO / 'docs/website-copy.md']

# 兜底：ASCII、中西标点、常见符号
EXTRA = ''.join(chr(c) for c in range(0x20, 0x7F)) + (
    '·—–…‘’“”《》〈〉「」『』（）【】、。！？：；，×÷±°→←↑↓✓✕≈≠≤≥　※'
)


def strip_comments(text: str) -> str:
    text = re.sub(r'/\*[\s\S]*?\*/', '', text)
    text = re.sub(r'<!--[\s\S]*?-->', '', text)
    # 只删整行注释：行内的 // 可能是 https:// 里的那两撇
    return re.sub(r'(?m)^\s*//.*$', '', text)


def collect() -> str:
    chars = set(EXTRA)
    for path in SITE + UI:
        if path.exists():
            chars |= set(strip_comments(path.read_text(encoding='utf-8')))
    for path in DOCS:
        if path.exists():
            chars |= set(path.read_text(encoding='utf-8'))
    chars = {c for c in chars if c.isprintable()}
    return ''.join(sorted(chars))


def subset(src: Path, dst: Path, text: str, variable: bool) -> None:
    subprocess.run(
        [
            sys.executable,
            '-m',
            'fontTools.subset',
            str(src),
            f'--output-file={dst}',
            '--flavor=woff2',
            f'--text={text}',
            '--layout-features=kern,liga,calt,ccmp,locl,mark,mkmk,palt',
            '--drop-tables+=DSIG',
            '--no-hinting',
            '--recalc-bounds' if variable else '--desubroutinize',
        ],
        check=True,
    )
    print(f'{dst.name:22} {dst.stat().st_size / 1024:7.1f} KB')


def main() -> None:
    src = Path(sys.argv[1] if len(sys.argv) > 1 else 'fonts')
    text = collect()
    print(f'共 {len(text)} 个字符，其中汉字 {len([c for c in text if "一" <= c <= "鿿"])} 个')
    OUT.mkdir(parents=True, exist_ok=True)

    # 拉丁：一个可变字体。opsz 14–32 由浏览器按字号自动取，
    # 大标题自动拿到 Inter Display 的那一头，不用另外挂一个家族。
    subset(src / 'InterVariable.ttf', OUT / 'inter-var.woff2', text, variable=True)
    # 中文：三个字重，正文 400、加粗 500、标题 700
    for weight, num in (('Regular', 400), ('Medium', 500), ('Bold', 700)):
        subset(src / f'HarmonyOS_Sans_SC_{weight}.ttf', OUT / f'harmony-{num}.woff2', text, False)


if __name__ == '__main__':
    main()
