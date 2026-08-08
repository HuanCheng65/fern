#!/usr/bin/env python3
"""用 `bash -n` 检查工作流里每一段 `run:` 的语法。

YAML 合法**不等于**里面的 shell 合法：一个少写的引号在 `yaml.safe_load` 眼里
只是字符串里的一个字符。这个错误真发生过（`"$base..HEAD` 少了个引号），
而 release.yml 里的同类错误只会在发版当天才暴露。

只查语法，不查语义——`bash -n` 不展开 `${{ }}`，所以先把它们换成一个占位符。
"""

import pathlib
import re
import subprocess
import sys

import yaml

# `${{ ... }}` 在 bash 眼里不是合法的东西。换成一个普通词，语法检查才做得下去；
# 这也意味着表达式本身的错误不在这里查——那是 GitHub 的事。
EXPRESSION = re.compile(r"\$\{\{[^}]*\}\}")


def run_blocks(document: dict):
    for job_name, job in (document.get("jobs") or {}).items():
        for index, step in enumerate(job.get("steps") or []):
            script = step.get("run")
            if not script:
                continue
            shell = step.get("shell", "bash")
            if shell not in ("bash", "sh"):
                continue
            yield f"{job_name}[{index}] {step.get('name', '(无名)')}", script


def main() -> int:
    failed = False
    checked = 0
    for path in sorted(pathlib.Path(".github/workflows").glob("*.yml")):
        document = yaml.safe_load(path.read_text())
        for where, script in run_blocks(document):
            checked += 1
            result = subprocess.run(
                ["bash", "-n"],
                input=EXPRESSION.sub("EXPR", script),
                capture_output=True,
                text=True,
            )
            if result.returncode != 0:
                print(f"{path}: {where}", file=sys.stderr)
                print(result.stderr.strip(), file=sys.stderr)
                failed = True
    if failed:
        return 1
    print(f"{checked} 段 run: 的 shell 语法没问题")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
