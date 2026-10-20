#!/usr/bin/env python3
"""从 Prism Launcher 的元数据里取出 Apple Silicon 要用的那几条库补丁。

1.18.2 及更早的版本只带 x86_64 的原生库（LWJGL 3.2.x 及以前、LWJGL 2），在
Apple Silicon 的原生 Java 上加载不起来。Prism 维护着一张「把这些库换成带
arm64 原生库的构建」的表；我们不另造一份，而是把它和 macOS 有关的条目原样
取过来，由 `fern-meta` 按 Prism 的语义应用（见 `fern-meta/src/patches.rs`）。

只取提到 `osx` 的条目：Linux ARM 那些在 macOS 上永远不会生效，留着只是让人读
不懂这份表在干什么。Prism 的条目之间会相互引用（`patchAdditionalLibraries`），
被引用的那几条本身也提到 `osx`，所以这个过滤不会断链。

用法：
    python3 .github/osx-arm64-library-patches.py <Prism meta 的提交号>

输出写到 fern-core/rules/library-patches-osx-arm64.json。来源与许可证（MS-PL）
记在文件头里。
"""

import json
import sys
import urllib.request
from pathlib import Path

SOURCE = "https://raw.githubusercontent.com/PrismLauncher/meta/{commit}/meta/common/mojang-library-patches.json"
OUTPUT = Path(__file__).resolve().parent.parent / "fern-core/rules/library-patches-osx-arm64.json"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    commit = sys.argv[1]
    with urllib.request.urlopen(SOURCE.format(commit=commit)) as response:
        patches = json.load(response)

    kept = [patch for patch in patches if '"osx' in json.dumps(patch)]
    document = {
        "_comment": (
            "Apple Silicon 上替换旧版 LWJGL 等库的补丁。由 .github/osx-arm64-library-patches.py "
            "从 Prism Launcher 的元数据生成，不要手改。"
        ),
        "source": {
            "repository": "https://github.com/PrismLauncher/meta",
            "path": "meta/common/mojang-library-patches.json",
            "commit": commit,
            "license": "MS-PL",
        },
        "patches": kept,
    }
    OUTPUT.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{len(kept)} / {len(patches)} 条 → {OUTPUT}")


if __name__ == "__main__":
    main()
