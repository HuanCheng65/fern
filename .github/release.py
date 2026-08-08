#!/usr/bin/env python3
"""发一个版本：改版本号、定稿更新日志、提交、打 tag。

    .github/release.py 0.2.0
    .github/release.py 0.2.0-beta.1
    .github/release.py 0.2.0 --dry-run

一条命令做完这些，因为手做要碰五个地方，而其中一个漏了不会有任何提示：

1. `fern-ui/src-tauri/Cargo.toml` 的 version
2. `fern-ui/src-tauri/tauri.conf.json` 的 version（漏了是编译错误，见 build.rs）
3. `fern-ui/src-tauri/Cargo.lock`
4. `CHANGELOG.md`：把「未发布」改成版本号和日期，并在上面留一个新的空「未发布」
5. tag `v<version>`（漏了或打错，CI 的 plan 会停）

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
CHANGELOG = ROOT / "CHANGELOG.md"
UNRELEASED = "## 未发布"

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
        fail(f"{CARGO} 里没有 version")
    CARGO.write_text("".join(lines))


def set_config_version(version: str) -> None:
    # 按文本改而不是 json.dump：那样会把整个文件重新格式化，diff 里全是无关行。
    text = CONFIG.read_text()
    before = json.loads(text)["version"]
    updated = text.replace(f'"version": "{before}"', f'"version": "{version}"', 1)
    if updated == text:
        fail(f"{CONFIG} 里的 version 没能替换")
    CONFIG.write_text(updated)


def close_changelog(version: str, today: str) -> None:
    """把「未发布」定稿成一个版本小节，并在上面留一个空的「未发布」。"""
    lines = CHANGELOG.read_text().splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == UNRELEASED)
    except StopIteration:
        fail(f"{CHANGELOG} 里没有「{UNRELEASED}」小节")
        return

    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")),
        len(lines),
    )
    body = [line for line in lines[start + 1 : end] if line.strip()]
    if not body or body == ["尚无面向用户的改动。"]:
        fail(
            "「未发布」小节是空的。先跑 .github/draft-changelog.py 汇总提交里的\n"
            "Release-Note，再读一遍生成的内容。真的没有面向用户的改动就手写一句。"
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
    parser = argparse.ArgumentParser()
    parser.add_argument("version", help="例如 0.2.0 或 0.2.0-beta.1")
    parser.add_argument("--dry-run", action="store_true", help="改文件但不提交不打 tag")
    parser.add_argument("--date", default=None, help="更新日志里的日期，默认今天")
    args = parser.parse_args()

    version = args.version.removeprefix("v")
    if not SEMVER.match(version):
        fail(f"{version!r} 不是合法的 SemVer")

    if run("git", "status", "--porcelain"):
        fail("工作区不干净。发版要从一个干净的树开始，否则 tag 指向的内容说不清。")

    now = current_version()
    if version == now:
        fail(f"当前已经是 {version}")
    tag = f"v{version}"
    if run("git", "tag", "--list", tag):
        fail(f"{tag} 已经存在。发布过的版本号不复用——路径里带版本号的产物是不可变的。")

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
    print(f"{now} → {version}（{channel} 通道，{today}）")

    if args.dry_run:
        print("--dry-run：文件已改，没有提交，也没有打 tag。")
        return 0

    subprocess.run(
        [
            "git", "commit", "--no-gpg-sign", "-a",
            "-m", f"chore(release): {version}",
        ],
        cwd=ROOT,
        check=True,
    )
    # 不签名：这个仓库的签名配置会让 git 卡住。
    subprocess.run(
        ["git", "tag", "-a", "--no-sign", tag, "-m", version], cwd=ROOT, check=True
    )
    print(f"\n已提交并打上 {tag}。推送即发布：\n  git push origin main --follow-tags")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
