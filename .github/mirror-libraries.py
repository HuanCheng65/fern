#!/usr/bin/env python3
"""把 Apple Silicon 库补丁表里挂在 GitHub 上的文件取下来，准备同步到 R2。

客户端的镜像源会把 `github.com/MinecraftMachina/...` 改写成
`<镜像站>/mirror/github.com/MinecraftMachina/...`（见 fern-download 的
`BmclapiSource`）。这里照同一个路径布局把文件放进 `--out` 目录，交给
`aws s3 cp --recursive` 上传。

已经在镜像站上的跳过：这些文件按版本号发布，内容不会变，重传只是浪费。每个
文件都按表里的 sha1 校验，对不上就失败——镜像上放一份错的，比没有镜像更糟。
"""

import argparse
import hashlib
import json
import pathlib
import sys
import urllib.error
import urllib.parse
import urllib.request

TABLE = pathlib.Path("fern-core/rules/library-patches-osx-arm64.json")
# 和 fern-download 里 FERN_MIRRORED_GITHUB 是同一份名单。
MIRRORED = ("/MinecraftMachina/",)


def files(document: dict):
    """表里所有的 (地址, sha1)，去重。"""
    seen = set()

    def walk(value):
        if isinstance(value, dict):
            if "url" in value and "sha1" in value:
                yield value["url"], value["sha1"]
            for child in value.values():
                yield from walk(child)
        elif isinstance(value, list):
            for child in value:
                yield from walk(child)

    for url, sha1 in walk(document["patches"]):
        if (url, sha1) not in seen:
            seen.add((url, sha1))
            yield url, sha1


def mirrored(url: str) -> bool:
    parsed = urllib.parse.urlparse(url)
    return parsed.netloc == "github.com" and parsed.path.startswith(MIRRORED)


def exists(url: str) -> bool:
    request = urllib.request.Request(url, method="HEAD")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status == 200
    except urllib.error.HTTPError:
        return False


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", required=True, help="镜像站，例如 https://dl.fern.huanchengfly.top")
    parser.add_argument("--out", required=True, type=pathlib.Path)
    args = parser.parse_args()

    document = json.loads(TABLE.read_text(encoding="utf-8"))
    staged = skipped = 0
    for url, sha1 in files(document):
        if not mirrored(url):
            continue
        key = "mirror/github.com" + urllib.parse.urlparse(url).path
        if exists(f"{args.base_url.rstrip('/')}/{key}"):
            skipped += 1
            continue
        with urllib.request.urlopen(url, timeout=120) as response:
            body = response.read()
        actual = hashlib.sha1(body).hexdigest()
        if actual != sha1.lower():
            print(f"::error::{url} 的 sha1 是 {actual}，表里写的是 {sha1}", file=sys.stderr)
            return 1
        target = args.out / key
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(body)
        staged += 1
        print(f"待上传 {key}")
    print(f"{staged} 个待上传，{skipped} 个已在镜像上。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
