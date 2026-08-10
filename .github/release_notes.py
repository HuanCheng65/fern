"""Release-Note 的写法规则。

两个地方要用同一批规则，所以规则放在这里：`check-release-notes.py` 查提交信息里的
尾注，`release.py` 查 `CHANGELOG.md`「未发布」小节里已经汇好的条目。两份各写一遍
必然会分叉，而分叉的那天不会有人发现。

这里只放机器判得准的部分。一句话写得好不好要人看；「写了两条」「用了 chore 类型」
「超了 60 字」不该靠人盯。
"""

import re

# 有用户可见变化的类型才该带尾注。其余类型改的是用户看不见的东西，一条属于 chore
# 的更新日志说明要么类型选错了，要么这条不该写。
#
# 前提不是「refactor 一定不改变界面」——重构顺手统一某个视觉细节很常见。是说：
# 这条变化值得写进更新日志的话，它就该有自己的提交。
TYPES_WITH_NOTES = {"feat", "fix", "perf"}

MAX_LENGTH = 60

# 更新日志是一列变化，不是一段文章，条目按惯例不带句末标点。规则朝这个方向定死，
# 是因为发布时 `build-manifest.py` 会把整节原样送进更新清单的 notes——半数带句号
# 半数不带，用户是看得出来的。
TRAILING_PUNCTUATION = "。．.；;，,！!"

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


def check_text(note: str) -> list[str]:
    """查一条更新日志本身。返回问题列表，空列表表示没问题。"""
    problems = []
    note = note.strip()

    if not note:
        return ["Release-Note 内容为空。"]
    if note[-1] in TRAILING_PUNCTUATION:
        problems.append(f"Release-Note 不以标点结尾，更新日志的条目是短句：「{note}」")
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
