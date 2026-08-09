#!/usr/bin/env python3
"""发一个版本：改版本号、定稿更新日志、提交、打 tag。

    .github/release.py 0.2.0
    .github/release.py 0.2.0-beta.1
    .github/release.py 0.2.0 --dry-run
    .github/release.py 0.2.0 --yes        # 不询问

一条命令做完这些，因为手做要碰五个地方，而其中一个漏了不会有任何提示：

1. `fern-ui/src-tauri/Cargo.toml` 的 version
2. `fern-ui/src-tauri/tauri.conf.json` 的 version（漏了是编译错误，见 build.rs）
3. `fern-ui/src-tauri/Cargo.lock`
4. `CHANGELOG.md`：把「未发布」改成版本号和日期，并在上面留一个新的空「未发布」
5. tag `v<version>`（漏了或打错，CI 的 plan 会停）

只碰上面这几个文件，也只提交这几个文件。仓库里其他没提交的东西——在写的官网、
还没定稿的文档——既不会挡住发版，也不会被带进 `chore(release)` 提交。但它们同样
不在这次的标签里，所以会被列出来并等一次明确确认：漏掉一个本该发出去的文件，
在流水线跑完之前不会有任何症状。非交互场景用 `--yes`。

**为什么不用 cargo-release。** 它是这件事的标准工具，但在这个仓库里要为三处
布局写自定义替换规则才能用：产品版本在一个嵌套的、`publish = false` 的独立
workspace 里，`tauri.conf.json` 要按 JSON 改而不是按正则，而 `CHANGELOG.md`
在仓库根。写完那些配置的篇幅和这个脚本差不多，而这个脚本能在本地真跑一遍。
如果哪天产物布局变简单了，换回 cargo-release 是对的。
"""

import argparse
import datetime
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CARGO = ROOT / "fern-ui/src-tauri/Cargo.toml"
CONFIG = ROOT / "fern-ui/src-tauri/tauri.conf.json"
LOCK = ROOT / "fern-ui/src-tauri/Cargo.lock"
CHANGELOG = ROOT / "CHANGELOG.md"
UNRELEASED = "## 未发布"

# 这次发布会重写、并且只提交这几个文件。仓库里的其他改动（在写的官网、还没
# 定稿的文档）既不该挡住发版，也不该被一个 `chore(release)` 提交顺手带上。
VERSION_FILES = [
    "fern-ui/src-tauri/Cargo.toml",
    "fern-ui/src-tauri/tauri.conf.json",
    "fern-ui/src-tauri/Cargo.lock",
]
# 更新日志不在上面那一组里：起草它是发版流程的第一步，跑到这里时它本来就是脏的。
RELEASE_FILES = ["CHANGELOG.md", *VERSION_FILES]

SEMVER = re.compile(
    r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def run(*args: str) -> str:
    return subprocess.run(
        args, cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()


def changed(paths: list[str]) -> list[str]:
    """这些路径里有未提交改动的那些（包括已暂存的）。"""
    # 不能用 run()：它会 strip 掉输出两端的空白，而 porcelain 的每一行都以
    # 两位状态码开头，未暂存的改动第一位就是空格——首行会因此少一个字符。
    status = subprocess.run(
        ["git", "status", "--porcelain", "--", *paths],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [line[3:] for line in status.splitlines() if line.strip()]


def check_scope() -> list[str]:
    """版本号文件必须干净。返回不属于本次发布的那些改动。"""
    dirty = changed(VERSION_FILES)
    if dirty:
        listed = "\n".join(f"  {path}" for path in dirty)
        fail(
            "以下文件有未提交的改动，而发版会重写它们：\n"
            f"{listed}\n"
            "请先提交或撤销这些改动。"
        )
    return sorted(set(changed(["."])) - set(RELEASE_FILES))


def confirm_scope(outside: list[str], assume_yes: bool) -> None:
    """把不进本次发布的改动列出来，等一次明确的确认。

    这里要的是「知情」而不是「许可」：漏提交一个文件，标签指向的就不是你以为的
    那份代码，而这件事在流水线跑完之前不会有任何症状。所以默认是不继续。
    """
    if not outside:
        return

    shown = outside[:20]
    listed = "\n".join(f"  {path}" for path in shown)
    more = f"\n  （另有 {len(outside) - len(shown)} 项）" if len(outside) > len(shown) else ""
    # 出错信息走 stderr，不刷一次的话输出被重定向时两边的顺序会颠倒。
    print(f"以下改动不属于本次发布，不会被提交，也不会进入标签：\n{listed}{more}", flush=True)

    if assume_yes:
        print("已跳过确认。\n")
        return
    if not sys.stdin.isatty():
        fail("需要确认，但当前不是交互式终端。确认无误后加上 --yes。")

    try:
        answer = input("确认这些改动不需要进入本次发布？[y/N] ").strip().lower()
    except (EOFError, KeyboardInterrupt):
        answer = ""
    if answer not in ("y", "yes"):
        fail("已取消。")
    print()


def current_version() -> str:
    for line in CARGO.read_text().splitlines():
        if line.startswith("version = "):
            return line.split('"')[1]
    fail(f"{CARGO} 里没有 version")
    return ""


def set_cargo_version(version: str) -> None:
    lines = CARGO.read_text().splitlines(keepends=True)
    for i, line in enumerate(lines):
        if line.startswith("version = "):
            lines[i] = f'version = "{version}"\n'
            break
    else:
        fail(f"{CARGO} 中没有 version 字段。")
    CARGO.write_text("".join(lines))


def set_config_version(version: str) -> None:
    # 按文本改而不是 json.dump：那样会把整个文件重新格式化，diff 里全是无关行。
    text = CONFIG.read_text()
    before = json.loads(text)["version"]
    updated = text.replace(f'"version": "{before}"', f'"version": "{version}"', 1)
    if updated == text:
        fail(f"未能替换 {CONFIG} 中的 version 字段。")
    CONFIG.write_text(updated)


def close_changelog(version: str, today: str) -> None:
    """把「未发布」定稿成一个版本小节，并在上面留一个空的「未发布」。"""
    lines = CHANGELOG.read_text().splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == UNRELEASED)
    except StopIteration:
        fail(f"{CHANGELOG} 中没有「{UNRELEASED}」小节。")
        return

    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")),
        len(lines),
    )
    # 只去掉首尾的空行。内部的空行要留着：小节里有 `### 新增` 这样的分类标题，
    # 去掉空行会让标题和列表挤在一起——而这段文字会原样进更新清单的 notes。
    body = lines[start + 1 : end]
    while body and not body[0].strip():
        body.pop(0)
    while body and not body[-1].strip():
        body.pop()

    if not body or [line for line in body if line.strip()] == ["尚无面向用户的改动。"]:
        fail(
            "「未发布」小节为空。先运行 .github/draft-changelog.py 汇总提交中的\n"
            "Release-Note 并确认生成的内容；如果本次确实没有面向用户的改动，\n"
            "请手动写入一句说明。"
        )

    # 原小节里的空行不能原样搬过来，否则版本标题下面会多出一行空白。
    lines[start:end] = [
        UNRELEASED,
        "",
        f"## {version} - {today}",
        "",
        *body,
        "",
    ]
    CHANGELOG.write_text("\n".join(lines).rstrip() + "\n")


def main() -> int:
    # 这个脚本的输出会和 git 自己的输出、以及 stderr 上的错误交替出现。默认的块
    # 缓冲只在终端里看着是对的，一重定向就乱序。
    sys.stdout.reconfigure(line_buffering=True)

    parser = argparse.ArgumentParser()
    parser.add_argument("version", help="版本号，例如 0.2.0 或 0.2.0-beta.1")
    parser.add_argument(
        "--dry-run", action="store_true", help="只修改文件，不提交，也不创建标签"
    )
    parser.add_argument("--date", default=None, help="更新日志中的发布日期，默认为今天")
    parser.add_argument(
        "--yes",
        action="store_true",
        help="不询问，直接确认「其余改动不进入本次发布」",
    )
    args = parser.parse_args()

    version = args.version.removeprefix("v")
    if not SEMVER.match(version):
        fail(f"{version} 不是合法的 SemVer 版本号。")

    outside = check_scope()

    now = current_version()
    if version == now:
        fail(f"当前版本已经是 {version}。")
    tag = f"v{version}"
    if run("git", "tag", "--list", tag):
        fail(
            f"标签 {tag} 已存在。版本号不重复使用：产物路径中带版本号，"
            "已发布的内容不可覆盖。"
        )

    # 确认放在所有能提前失败的检查之后：先问一遍再报「标签已存在」是在浪费人的时间。
    confirm_scope(outside, assume_yes=args.yes or args.dry_run)

    today = args.date or datetime.date.today().isoformat()
    close_changelog(version, today)
    set_cargo_version(version)
    set_config_version(version)
    subprocess.run(
        ["cargo", "metadata", "--manifest-path", str(CARGO), "--format-version", "1"],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        check=True,
    )

    channel = "beta" if "-" in version else "stable"
    print(f"{now} → {version}（{channel} 通道，发布日期 {today}）")

    if args.dry_run:
        print("--dry-run：已修改文件，未提交，也未创建标签。")
        return 0

    # 按路径提交，不用 `-a`：`-a` 会把仓库里其他所有已跟踪文件的改动一并带上。
    subprocess.run(
        [
            "git", "commit", "--no-gpg-sign",
            "-m", f"chore(release): {version}",
            "--", *RELEASE_FILES,
        ],
        cwd=ROOT,
        check=True,
    )
    # 不签名：这个仓库的签名配置会让 git 卡住。
    subprocess.run(
        ["git", "tag", "-a", "--no-sign", tag, "-m", version], cwd=ROOT, check=True
    )
    print(f"\n已提交并创建标签 {tag}。推送后开始发布：\n  git push origin main --follow-tags")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
