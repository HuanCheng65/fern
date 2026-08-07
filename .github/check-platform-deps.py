#!/usr/bin/env python3
"""每个目标平台解析出来的依赖必须一致（平台专属的那几个除外）。

起因：往 `fern-core/Cargo.toml` 里插一个 `[target.'cfg(unix)'.dependencies]`
段时，位置插错了，它后面的九个依赖（serde、tokio、reqwest……）全被归进了
unix 分支。Linux 和 macOS 上 `cfg(unix)` 为真，照常编译；Windows 上它们集体
消失，报出来是「unresolved import」刷屏——而错误指向源码，看不出是清单的问题。

这类错误只有交叉编译或真机构建才暴露，而那要十几分钟。依赖解析不需要编译，
几秒钟就能跑完，所以放在质量检查里当第一道闸。
"""

import json
import subprocess
import sys

TARGETS = [
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
]

# 有意只在某些平台存在的。新增平台专属依赖时要同时加到这里，否则这个检查会
# 把它当成事故——那正是我们想要的默认行为。
PLATFORM_SPECIFIC = {"libc", "windows_sys", "windows-sys"}


def dependencies_for(target: str) -> dict[str, set[str]]:
    raw = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", target],
        capture_output=True,
        check=True,
        text=True,
    ).stdout
    metadata = json.loads(raw)
    local = {
        package["id"]: package["name"]
        for package in metadata["packages"]
        if package["name"].startswith("fern-")
    }
    result: dict[str, set[str]] = {}
    for node in metadata["resolve"]["nodes"]:
        name = local.get(node["id"])
        if name is None:
            continue
        result[name] = {
            dep["name"] for dep in node["deps"] if dep["name"] not in PLATFORM_SPECIFIC
        }
    return result


def main() -> int:
    per_target = {target: dependencies_for(target) for target in TARGETS}
    baseline_target = TARGETS[0]
    baseline = per_target[baseline_target]

    failed = False
    for target in TARGETS[1:]:
        for crate, deps in baseline.items():
            other = per_target[target].get(crate, set())
            missing = deps - other
            extra = other - deps
            if missing or extra:
                failed = True
                print(f"{crate} 在 {target} 上和 {baseline_target} 不一致：")
                if missing:
                    print(f"  缺少：{', '.join(sorted(missing))}")
                if extra:
                    print(f"  多出：{', '.join(sorted(extra))}")

    if failed:
        print()
        print("多半是某个 [target.'cfg(...)'.dependencies] 段的位置写错了，")
        print("把它后面本该跨平台的依赖一起圈了进去。")
        return 1

    summary = "、".join(f"{crate} {len(deps)} 项" for crate, deps in sorted(baseline.items()))
    print(f"{len(TARGETS)} 个目标平台的依赖一致：{summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
