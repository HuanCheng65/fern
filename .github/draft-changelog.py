#!/usr/bin/env python3
"""把提交里的 `Release-Note:` 尾注汇成 CHANGELOG.md 的「未发布」小节。

**这是起草，不是定稿。** 生成之后要读一遍：措辞是不是人话、顺序合不合理、
两条能不能并成一条。工具能保证的只是「写过的不会丢」。

用法：

    draft-changelog.py            # 从最近一个 tag 到 HEAD
    draft-changelog.py <range>
    draft-changelog.py --check    # 只报告有没有变化，不写文件（CI 用）
"""

import argparse
import pathlib
import re
import subprocess
import sys

UNRELEASED = "## 未发布"

# 提交类型 → 更新日志里的分类。顺序就是小节里的顺序：先说多了什么，
# 再说变好了什么，最后说修了什么。
CATEGORIES = [("feat", "新增"), ("perf", "改进"), ("fix", "修复")]

SUBJECT = re.compile(r"^(?P<type>[a-z]+)(?:\([^)]*\))?!?: ")
TRAILER = re.compile(r"^Release-Note:\s*(?P<note>.+)$", re.MULTILINE)


def collect(revision_range: str) -> dict[str, list[str]]:
    """按分类收集尾注。同一句话只留一条。"""
    # 分隔符交给 git 自己转义（`%x00`）：把真的 NUL 字节拼进 argv 是非法的。
    separator = "\x00"
    out = subprocess.run(
        ["git", "log", "--reverse", "--format=%B%x00", revision_range],
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    found: dict[str, list[str]] = {}
    for chunk in out.split(separator):
        message = chunk.strip()
        if not message:
            continue
        note = TRAILER.search(message)
        if not note:
            continue
        subject = message.splitlines()[0]
        match = SUBJECT.match(subject)
        kind = match.group("type") if match else "fix"
        label = dict(CATEGORIES).get(kind)
        if label is None:
            continue
        text = note.group("note").strip()
        if text not in found.setdefault(label, []):
            found[label].append(text)
    return found


def render(found: dict[str, list[str]]) -> str:
    lines = [UNRELEASED, ""]
    for _, label in CATEGORIES:
        notes = found.get(label)
        if not notes:
            continue
        lines.append(f"### {label}")
        lines.append("")
        lines.extend(f"- {note}" for note in notes)
        lines.append("")
    if len(lines) == 2:
        lines.append("尚无面向用户的改动。")
        lines.append("")
    return "\n".join(lines)


def replace_section(changelog: str, section: str) -> str:
    """换掉「未发布」那一节；没有就插在第一个版本小节之前。"""
    lines = changelog.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == UNRELEASED)
    except StopIteration:
        try:
            start = next(i for i, line in enumerate(lines) if line.startswith("## "))
        except StopIteration:
            start = len(lines)
        return "\n".join(lines[:start] + section.splitlines() + lines[start:]) + "\n"

    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")),
        len(lines),
    )
    body = section.splitlines()
    # 和下一个版本小节之间留一个空行。`splitlines()` 会把 section 末尾那个
    # 空行吃掉，不补的话生成出来是「- 最后一条」紧贴着「## 0.1.0」。
    if end < len(lines) and body and body[-1] != "":
        body.append("")
    return "\n".join(lines[:start] + body + lines[end:]) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("range", nargs="?", default=None)
    parser.add_argument("--changelog", type=pathlib.Path, default=pathlib.Path("CHANGELOG.md"))
    parser.add_argument(
        "--check",
        action="store_true",
        help="不写文件，只在内容需要更新时以非零码退出。",
    )
    args = parser.parse_args()

    revision_range = args.range
    if revision_range is None:
        last = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0"], capture_output=True, text=True
        )
        revision_range = f"{last.stdout.strip()}..HEAD" if last.returncode == 0 else "HEAD"

    section = render(collect(revision_range))
    before = args.changelog.read_text() if args.changelog.exists() else "# 更新日志\n\n"
    after = replace_section(before, section)

    if args.check:
        if before != after:
            print("CHANGELOG.md 的「未发布」小节和提交里的 Release-Note 对不上。", file=sys.stderr)
            print("跑一次 .github/draft-changelog.py 再读一遍生成的内容。", file=sys.stderr)
            return 1
        print("「未发布」小节是最新的")
        return 0

    args.changelog.write_text(after)
    print(section)
    print(f"已写入 {args.changelog}（{revision_range}）。**读一遍再提交。**")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
