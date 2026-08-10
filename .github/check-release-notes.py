#!/usr/bin/env python3
"""校验提交里的 `Release-Note:` 尾注。规范在 AGENTS.md 的「更新日志」一节。

**这个检查在哪儿挡，比它查什么更重要。** 提交信息推出去之后就是只读的：在 CI 里
让它失败，等于规定「少个标点的唯一出路是 rebase 加强推」，代价和收益完全不成比例。
所以分三处：

- `commit-msg` 钩子（`.githooks/`）——写的时候就挡。这时候改一条信息是免费的，
  所以这里是硬的。
- CI（`--warn`）——只提醒，不挡。推上去才发现的问题，留给下面那一关。
- 发版（`release.py`）——真正的关口。那时候要审的是 `CHANGELOG.md` 里已经汇好的
  文字，改它只是改一个文件，而它就是要发给用户的东西。

用法：

    check-release-notes.py <base>..<head>
    check-release-notes.py                    # 默认查最近一个 tag 到 HEAD
    check-release-notes.py --warn <range>     # 只提醒，始终以 0 退出
    check-release-notes.py --message-file <path>   # 查单条提交信息（钩子用）
"""

import argparse
import pathlib
import re
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from release_notes import TYPES_WITH_NOTES, check_text  # noqa: E402

SUBJECT = re.compile(r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?: ")
TRAILER = re.compile(r"^Release-Note:\s*(?P<note>.*)$", re.MULTILINE)

# `git commit` 交给钩子的文件里还带着模板注释，verbose 模式下后面还跟着整个 diff。
SCISSORS = "# ------------------------ >8 ------------------------"


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


def strip_comments(raw: str) -> str:
    """去掉 git 加在提交信息文件里的注释和 diff。"""
    lines = []
    for line in raw.splitlines():
        if line.rstrip() == SCISSORS:
            break
        if line.startswith("#"):
            continue
        lines.append(line)
    return "\n".join(lines).strip()


def check(message: str) -> list[str]:
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

    kind = match.group("type") if match else None
    if kind is None:
        problems.append("提交标题不符合 Conventional Commits，无法确定更新日志的分类。")
    elif kind not in TYPES_WITH_NOTES:
        problems.append(
            f"提交类型为 {kind}，其改动对用户不可见，不应包含 Release-Note。"
            f"（可包含的类型：{'、'.join(sorted(TYPES_WITH_NOTES))}）"
        )

    return problems + check_text(notes[0])


def report(problems: list[tuple[str, str]], warn_only: bool) -> int:
    if not problems:
        return 0
    for label, problem in problems:
        prefix = "::warning::" if warn_only else ""
        print(f"{prefix}{label}{problem}", file=sys.stderr)
    print("\n写法规范见 AGENTS.md 的「更新日志」一节。", file=sys.stderr)
    if warn_only:
        print(
            "这里只提醒——提交信息已经推出去了，改它要改写历史。发版时 release.py "
            "会拿 CHANGELOG.md 里的条目再查一遍，那时候改的是文件。",
            file=sys.stderr,
        )
        return 0
    return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("range", nargs="?", default=None)
    parser.add_argument(
        "--warn",
        action="store_true",
        help="只报告问题，始终以 0 退出（CI 用）。",
    )
    parser.add_argument(
        "--message-file",
        type=pathlib.Path,
        default=None,
        help="查一个提交信息文件而不是一段提交范围（commit-msg 钩子用）。",
    )
    args = parser.parse_args()

    if args.message_file is not None:
        message = strip_comments(args.message_file.read_text())
        return report([("", problem) for problem in check(message)], args.warn)

    revision_range = args.range
    if revision_range is None:
        last = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0"],
            capture_output=True,
            text=True,
        )
        revision_range = f"{last.stdout.strip()}..HEAD" if last.returncode == 0 else "HEAD"

    problems = []
    checked = 0
    for short, message in commits(revision_range):
        checked += 1
        problems.extend((f"{short}: ", problem) for problem in check(message))

    if not problems:
        print(f"已检查 {checked} 个提交的 Release-Note（范围 {revision_range}），未发现问题。")
    return report(problems, args.warn)


if __name__ == "__main__":
    raise SystemExit(main())
