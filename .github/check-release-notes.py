#!/usr/bin/env python3
"""校验提交里的 `Release-Note:` 尾注。

规范在 AGENTS.md 的「更新日志」一节。这里只查机器查得动的部分——一句话写得好不好
要人看，但「写了两条」「用了 chore 类型」「结尾少个句号」这些不该靠人盯。

用法：

    check-release-notes.py <base>..<head>
    check-release-notes.py            # 默认查最近一个 tag 到 HEAD
"""

import re
import subprocess
import sys

# 有用户可见变化的类型才该带尾注。其余类型改的是用户看不见的东西，
# 一条属于 chore 的更新日志说明要么类型选错了，要么这条不该写。
TYPES_WITH_NOTES = {"feat", "fix", "perf"}
MAX_LENGTH = 60

SUBJECT = re.compile(r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?: ")
TRAILER = re.compile(r"^Release-Note:\s*(?P<note>.*)$", re.MULTILINE)

# 没有信息量的句子。写不出具体是什么，就说明这条不该进更新日志。
EMPTY_PHRASES = [
    "优化了体验",
    "优化体验",
    "优化了使用体验",
    "提升了稳定性",
    "提升稳定性",
    "修复了一些",
    "修复一些",
    "已知问题",
    "若干问题",
    "其他改进",
    "细节优化",
]


def commits(revision_range: str) -> list[tuple[str, str]]:
    """(短哈希, 完整提交信息) 的列表。"""
    # 分隔符交给 git 自己转义（`%x00`）。把真的 NUL 字节拼进 argv 是非法的，
    # execve 会直接拒绝。
    separator = "\x00"
    out = subprocess.run(
        ["git", "log", "--format=%h%x1f%B%x00", revision_range],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    result = []
    for chunk in out.split(separator):
        if not chunk.strip():
            continue
        short, _, message = chunk.strip().partition("\x1f")
        result.append((short, message))
    return result


def check(short: str, message: str) -> list[str]:
    problems = []
    subject = message.splitlines()[0] if message.splitlines() else ""
    match = SUBJECT.match(subject)
    notes = TRAILER.findall(message)

    if len(notes) > 1:
        problems.append(
            f"包含 {len(notes)} 条 Release-Note，每个提交最多一条。"
            "需要两条时，应拆分为两个提交。"
        )
    if not notes:
        return problems

    note = notes[0].strip()
    kind = match.group("type") if match else None

    if kind is None:
        problems.append("提交标题不符合 Conventional Commits，无法确定更新日志的分类。")
    elif kind not in TYPES_WITH_NOTES:
        problems.append(
            f"提交类型为 {kind}，其改动对用户不可见，不应包含 Release-Note。"
            f"（可包含的类型：{'、'.join(sorted(TYPES_WITH_NOTES))}）"
        )

    if not note:
        problems.append("Release-Note 内容为空。")
        return problems
    if not note.endswith("。"):
        problems.append(f"Release-Note 应以句号结尾：「{note}」")
    if len(note) > MAX_LENGTH:
        problems.append(
            f"Release-Note 超过 {MAX_LENGTH} 个字符（当前 {len(note)} 个）：「{note}」"
        )
    if not re.search(r"[一-鿿]", note):
        problems.append(f"Release-Note 应使用中文：「{note}」")
    for phrase in EMPTY_PHRASES:
        if phrase in note:
            problems.append(
                f"「{phrase}」不具体。Release-Note 应说明具体改动，"
                "无法说明的改动不应写入更新日志。"
            )
            break
    return problems


def main() -> int:
    if len(sys.argv) > 1:
        revision_range = sys.argv[1]
    else:
        last = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0"],
            capture_output=True,
            text=True,
        )
        revision_range = f"{last.stdout.strip()}..HEAD" if last.returncode == 0 else "HEAD"

    failed = False
    checked = 0
    for short, message in commits(revision_range):
        checked += 1
        for problem in check(short, message):
            print(f"{short}: {problem}", file=sys.stderr)
            failed = True

    if failed:
        print("\n写法规范见 AGENTS.md 的「更新日志」一节。", file=sys.stderr)
        return 1
    print(f"已检查 {checked} 个提交的 Release-Note（范围 {revision_range}），未发现问题。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
