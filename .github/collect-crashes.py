#!/usr/bin/env python3
"""抓一批真实的 Minecraft 崩溃报告，脱敏后落到本地语料目录。

    python3 .github/collect-crashes.py corpus/            # 抓
    python3 .github/collect-crashes.py --self-test        # 只验脱敏

语料**不进仓库**：几百份别人的日志没必要压进 git。进仓库的只有支撑某一条规则
的那一份，洗干净之后放进 `fern-core/rules/fixtures/`。用
`cargo run -p fern-core --example crash_coverage -- corpus/` 看覆盖率。

GitHub 的代码搜索单次查询最多返回 1000 条，所以本来就得切分。按 **MC 版本**切
一举两得：切分的同时，语料是按版本分层的，而不是碰运气——「版本太多」这个担心
就是这么解决的。

**脱敏在入库前做，不是入库后。** 日志里除了玩家名、UUID、家目录路径，有时还有
完整的启动命令行，里面带 access token。漏一次就是把别人的令牌提交进公开仓库。
"""

import argparse
import concurrent.futures
import json
import pathlib
import re
import subprocess
import sys
import time

MARKER = "---- Minecraft Crash Report ----"

# 每一档抓一批，语料因此按版本分层。想要哪些版本就改这里。
VERSIONS = [
    "1.7.10", "1.12.2", "1.16.5", "1.18.2",
    "1.19.2", "1.20.1", "1.20.4", "1.21.1", "1.21.4",
]

# 顺序有讲究：先抹长的（整条命令行），再抹短的（单个字段），否则短的会把长的
# 拆开，剩下的碎片反而漏出去。
SCRUBBERS = [
    # 启动命令行里的令牌。这一条最要紧。
    (re.compile(r"(--accessToken|--session)\s+\S+"), r"\1 <token>"),
    (re.compile(r'("?access_?[Tt]oken"?\s*[:=]\s*"?)[\w.\-]{16,}'), r"\1<token>"),
    (re.compile(r"(--uuid|--username)\s+\S+"), r"\1 <redacted>"),
    # Windows / macOS / Linux 的家目录。
    (re.compile(r"([A-Za-z]:\\Users\\)[^\\\s]+"), r"\1<user>"),
    (re.compile(r"(/Users/)[^/\s]+"), r"\1<user>"),
    (re.compile(r"(/home/)[^/\s]+"), r"\1<user>"),
    # UUID 与 IP。
    (re.compile(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"), "<uuid>"),
    (re.compile(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b"), "<ip>"),
    # 崩溃报告里明写的玩家名。
    (re.compile(r"(Minecraft Username:\s*).+"), r"\1<player>"),
    (re.compile(r"(Setting user:\s*).+"), r"\1<player>"),
]


def scrub(text: str) -> str:
    for pattern, replacement in SCRUBBERS:
        text = pattern.sub(replacement, text)
    return text


def self_test() -> int:
    cases = [
        ("--accessToken eyJhbGciOi.AAAA.BBBB --uuid 1234", "eyJhbGciOi"),
        (r"at C:\Users\张三\AppData\Roaming", "张三"),
        ("/home/player/.minecraft/mods", "/home/player"),
        ("Minecraft Username: SomeoneReal", "SomeoneReal"),
        ("uuid 069a79f4-44e9-4726-a5be-fca90e38aaf5 here", "069a79f4"),
        ("connecting to 192.168.31.44:25565", "192.168.31.44"),
        ('"accessToken": "abcdefghijklmnopqrstuvwxyz"', "abcdefghijklmnop"),
    ]
    failures = 0
    for original, secret in cases:
        cleaned = scrub(original)
        if secret in cleaned:
            print(f"没抹掉：{secret!r} 仍在 {cleaned!r}")
            failures += 1
    print("脱敏自检通过" if not failures else f"{failures} 项失败")
    return 1 if failures else 0


# 代码搜索的配额很紧（每分钟十次上下），翻页之间必须歇一会儿，否则后面几档
# 全部空手而回。
SEARCH_PAUSE = 7.0


def search(query: str, per_page: int = 100, pages: int = 3):
    """GitHub 代码搜索。要 `gh auth login`。"""
    for page in range(1, pages + 1):
        if page > 1:
            time.sleep(SEARCH_PAUSE)
        result = subprocess.run(
            ["gh", "api", "-X", "GET", "search/code",
             "-f", f"q={query}", "-f", f"per_page={per_page}", "-f", f"page={page}"],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            # 配额用完或翻到头了，换下一档。
            print(f"  停在第 {page} 页：{result.stderr.strip().splitlines()[:1]}")
            return
        items = json.loads(result.stdout).get("items", [])
        if not items:
            return
        yield from items


def raw_url(item: dict) -> str:
    return (item["html_url"]
            .replace("https://github.com/", "https://raw.githubusercontent.com/")
            .replace("/blob/", "/"))


def fetch_one(job) -> bool:
    """下一份，脱敏，落盘。返回是否真的存下了。"""
    url, path = job
    if path.exists():
        return False
    fetched = subprocess.run(["curl", "-sSL", "--max-time", "30", url],
                             capture_output=True, text=True)
    text = fetched.stdout
    # 只要真的是崩溃报告。
    if MARKER not in text:
        return False
    path.write_text(scrub(text), encoding="utf-8")
    return True


def collect(target: pathlib.Path, pages: int, workers: int) -> int:
    target.mkdir(parents=True, exist_ok=True)
    jobs = {}
    for version in VERSIONS:
        query = f'"{MARKER}" "Minecraft Version: {version}" path:crash-reports'
        found = 0
        for item in search(query, pages=pages):
            # 按 blob 的 sha 去重：同一份报告常常被 fork 到好几个仓库。
            jobs[item["sha"]] = (raw_url(item), target / f"{version}-{item['sha'][:12]}.txt")
            found += 1
        print(f"  {version}：搜到 {found}")
        time.sleep(SEARCH_PAUSE)

    print(f"去重后 {len(jobs)} 份，开始下载…")
    saved = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        for ok in pool.map(fetch_one, jobs.values()):
            saved += 1 if ok else 0
    print(f"存了 {saved} 份到 {target}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", nargs="?", default="corpus")
    parser.add_argument("--self-test", action="store_true", help="只验脱敏规则")
    parser.add_argument("--pages", type=int, default=3, help="每个版本翻几页，一页 100 条")
    parser.add_argument("--workers", type=int, default=16, help="同时下几份")
    arguments = parser.parse_args()
    if arguments.self_test:
        return self_test()
    return collect(pathlib.Path(arguments.target), arguments.pages, arguments.workers)


if __name__ == "__main__":
    sys.exit(main())
