#!/usr/bin/env python3
"""把一次发布的产物写成一份 manifest.json。

清单有**两个读者**，这是这个脚本存在的全部理由：

- `fern-core/src/update/` 读 `rollout` / `critical` / `minVersion`，决定这台机器
  要不要更新；
- `tauri-plugin-updater` 读 `version` / `platforms`，去下载和验签。

后者的字段名不能改——那是它定的。前者的字段它不认识，但也不会因此报错
（`RemoteRelease` 没有 `deny_unknown_fields`），所以两份内容可以住在同一个文件里。
拆成两个文件的话，它们会漂移，而漂移的那一天没人会注意到。

`rollout` 默认写 100。「发了一版但一个人也收不到」是比「发得太快」更坏的失败：
前者会安静地什么都不发生，而发版的人以为自己已经发过了。要灰度就发完之后把
这个数字改小——它是清单里唯一一个可以事后单独改的字段。
"""

import argparse
import json
import pathlib
import sys

# 平台键 → (产物文件名, 签名文件名)。
#
# macOS 是 universal 包，两个架构指向同一个文件——这不是偷懒，那个包里真的两份都有。
# 键名必须和 Tauri 更新器算出来的一致（`{os}-{arch}`，macOS 叫 darwin），
# 也必须和 `fern_core::update::target()` 一致。三处对不上的症状都是「说有更新，
# 下载时说没有这个平台」。
PLATFORMS = {
    "windows-x86_64": "Fern-windows-x86_64.exe",
    "darwin-aarch64": "Fern-darwin-universal.app.tar.gz",
    "darwin-x86_64": "Fern-darwin-universal.app.tar.gz",
    "linux-x86_64": "Fern-linux-x86_64.AppImage",
}


def notes_for(changelog: pathlib.Path, version: str) -> str | None:
    """取 CHANGELOG.md 里属于这个版本的那一节。

    找不到就返回 None，**不报错**：一次没写更新日志的发布仍然应该发得出去。
    界面在没有 notes 时什么都不显示，而不是显示一句「暂无更新日志」——
    那句话占着位置却没有信息。
    """
    if not changelog.exists():
        return None

    def is_wanted(line: str) -> bool:
        # 标题是「## <版本号> - <日期>」，日期由 release.py 补上；早期几节只有版本号。
        # 精确匹配整行会漏掉带日期的那种——症状是发布出去的清单没有 notes。
        if not line.startswith("## "):
            return False
        return line[3:].strip().split(" ", 1)[0] == version

    lines = changelog.read_text().splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if is_wanted(line))
    except StopIteration:
        return None
    body = []
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        body.append(line)
    text = "\n".join(body).strip()
    return text or None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--files", required=True, type=pathlib.Path)
    parser.add_argument("--out", required=True, type=pathlib.Path)
    parser.add_argument(
        "--changelog",
        type=pathlib.Path,
        default=pathlib.Path("CHANGELOG.md"),
        help="从该文件中取本版本小节作为更新日志。",
    )
    parser.add_argument(
        "--notes", default=None, help="直接指定更新日志文本，覆盖 --changelog。"
    )
    parser.add_argument(
        "--require-notes",
        action="store_true",
        help="缺少更新日志时失败。正式版发布应当启用。",
    )
    parser.add_argument(
        "--rollout",
        type=int,
        default=100,
        help="放量百分比，默认 100，即全量。",
    )
    parser.add_argument("--critical", action="store_true")
    parser.add_argument("--min-version", default=None)
    args = parser.parse_args()

    if not 0 <= args.rollout <= 100:
        print(f"--rollout 应在 0 到 100 之间，当前为 {args.rollout}。", file=sys.stderr)
        return 1

    base = args.base_url.rstrip("/")
    platforms = {}
    for key, name in PLATFORMS.items():
        binary = args.files / name
        signature = args.files / f"{name}.sig"
        # 少一个平台就整份失败，不生成一份「大部分平台能用」的清单：
        # 那样缺的那个平台上的用户会看到「这个平台还没有构建」，
        # 而真相是这一次发布出了错。
        if not binary.exists():
            print(f"缺少产物文件：{binary}", file=sys.stderr)
            return 1
        if not signature.exists():
            print(f"缺少签名文件：{signature}", file=sys.stderr)
            return 1
        platforms[key] = {
            "url": f"{base}/release/{args.version}/{name}",
            "signature": signature.read_text().strip(),
        }

    manifest = {
        "version": args.version,
        "rollout": args.rollout,
        "critical": args.critical,
        "platforms": platforms,
    }
    notes = args.notes or notes_for(args.changelog, args.version)
    if not notes and args.require_notes:
        print(
            f"{args.changelog} 中没有 ## {args.version} 小节。\n"
            "正式版必须提供更新日志，其内容会显示在设置的「关于」页面。\n"
            "先运行 .github/draft-changelog.py 汇总提交中的 Release-Note。",
            file=sys.stderr,
        )
        return 1
    if notes:
        manifest["notes"] = notes
    if args.min_version:
        manifest["minVersion"] = args.min_version

    args.out.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
