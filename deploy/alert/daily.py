#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""每日日报:请求量 / 5xx / 活跃服务 / 磁盘 → 日报群。
用法:python3 daily.py [--dry-run]
"""
import json
import os
import subprocess
import sys
from datetime import datetime, timedelta

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from send_feishu import send  # noqa: E402


def load_config():
    cfg = {}
    with open(os.path.join(HERE, "config.env")) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            cfg[k.strip()] = v.strip().strip('"').strip("'")
    return cfg


def run(cmd):
    try:
        r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=30)
        return r.stdout
    except Exception as e:
        return f"<error: {e}>"


def main():
    cfg = load_config()
    dry = "--dry-run" in sys.argv
    stack = cfg.get("STACK_NAME", "combee")
    since = (datetime.utcnow() - timedelta(hours=24)).strftime("%Y-%m-%dT%H:%M")

    logs = run(
        f"docker service logs --since {since} {stack}_api-server --no-trunc 2>&1"
    )
    statuses = [int(x) for x in re.findall(r'"status":(\d+)', logs) if x.isdigit()]
    total = len(statuses)
    fivexx = sum(1 for c in statuses if c >= 500)
    fourxx = sum(1 for c in statuses if 400 <= c < 500)
    err_rate = f"{fivexx / total * 100:.1f}%" if total else "-"

    svc_out = run(f"docker service ls --format '{{{{.Name}}}} {{{{.Replicas}}}}' | grep '^{stack}_'")
    svc_lines = [l for l in svc_out.splitlines() if "/" in l] or ["<无服务>"]

    disk = run("df -P / | tail -1").split()
    disk_str = f"{disk[4]}({disk[3]} 可用)" if len(disk) >= 5 else "-"

    today = datetime.now().strftime("%Y-%m-%d")
    lines = [
        f"**Combee 日报 {today}**(最近 24h)",
        "",
        f"- API 请求: **{total}**",
        f"- 5xx: **{fivexx}**(占比 {err_rate})",
        f"- 4xx: **{fourxx}**",
        f"- 磁盘: {disk_str}",
        "",
        "服务状态:",
        *[f"  - {l}" for l in svc_lines[:8]],
    ]
    content = "\n".join(lines)
    if dry:
        print(content)
        return
    send(
        cfg.get("FEISHU_DAILY_WEBHOOK", ""),
        f"Combee 日报 {today}",
        content,
        "P2",
        cfg.get("FEISHU_DAILY_SECRET", "") or None,
    )


import re  # noqa: E402


if __name__ == "__main__":
    main()
