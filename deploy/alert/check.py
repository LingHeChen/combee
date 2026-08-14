#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Combee 服务器告警检查(轻量方案:docker + 磁盘 + 证书 + API 可用性 + 日志错误)。

用法:
  python3 check.py                 # 全量检查并发送告警
  python3 check.py --dry-run       # 只打印会发送的内容,不发消息

部署:配合 cron 每 5 分钟执行一次(见 README.md)。
去重:同一告警项 COOLDOWN_MIN 内不重复;条件恢复后发送"已恢复"通知。

覆盖(对齐 artifacts/COMBEE_OBSERVABILITY_ALERTING_PLAN.md §31):
  P0 服务副本不足 / 磁盘临界 / 证书将过期 / API 不可达 / 5xx 率过高
  P1 磁盘预警 / 证书预警 / API 5xx / 服务 ERROR 量 / 认证失败突增 / 5xx 率升高 / 备份连续失败
  P2 资源水位(CPU/内存)/ 慢请求 / 备份单次失败
"""
import json
import os
import re
import re
import subprocess
import sys
import time
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from send_feishu import send  # noqa: E402

STATE_FILE = "/tmp/combee-alert-state.json"


def load_config():
    cfg = {}
    path = os.path.join(HERE, "config.env")
    if not os.path.exists(path):
        print("[error] 缺少 config.env(先 cp config.example.env config.env)", file=sys.stderr)
        sys.exit(1)
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            cfg[k.strip()] = v.strip().strip('"').strip("'")
    return cfg


def load_state():
    try:
        with open(STATE_FILE) as f:
            return json.load(f)
    except Exception:
        return {}


def save_state(state):
    with open(STATE_FILE, "w") as f:
        json.dump(state, f)


def cooled(cfg, state, key):
    last = state.get(key, {}).get("last", 0)
    cooldown = int(cfg.get("COOLDOWN_MIN", 30)) * 60
    return (time.time() - last) < cooldown


def run(cmd):
    try:
        r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=30)
        return r.stdout
    except Exception as e:
        print("[error] 执行失败: %s -> %s" % (cmd, e), file=sys.stderr)
        return ""


def service_health(cfg):
    stack = cfg.get("STACK_NAME", "combee")
    out = run("docker service ls --format '{{.Name}} {{.Replicas}}' | grep '^%s_'" % stack)
    problems = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        name, rep = parts[0], parts[1]
        m = re.match(r"(\d+)/(\d+)", rep)
        if m and int(m.group(1)) < int(m.group(2)):
            problems.append("- %s: 副本 %s" % (name, rep))
    return ("P0", "\n".join(problems)) if problems else None


def disk_usage(cfg):
    warn, crit = int(cfg.get("DISK_WARN", 85)), int(cfg.get("DISK_CRIT", 92))
    out = run("df -P / | tail -1")
    parts = out.split()
    if len(parts) < 5:
        return None
    pct = int(parts[4].rstrip("%"))
    if pct >= crit:
        return ("P0", "磁盘使用率 %s%%(临界 %s%%),可用 %s" % (pct, crit, parts[3]))
    if pct >= warn:
        return ("P1", "磁盘使用率 %s%%(预警 %s%%)" % (pct, warn))
    return None


def cert_expiry(cfg):
    crit = int(cfg.get("CERT_CRIT", 7))
    warn = int(cfg.get("CERT_WARN", 30))
    domains = cfg.get("DOMAINS", "").split()
    problems = []
    for d in domains:
        out = run(
            "echo | openssl s_client -servername %s -connect %s:443 2>/dev/null "
            "| openssl x509 -noout -enddate 2>/dev/null | cut -d= -f2" % (d, d)
        )
        if not out.strip():
            continue
        try:
            end = datetime.strptime(out.strip(), "%b %d %H:%M:%S %Y %Z")
        except ValueError:
            continue
        days = (end - datetime.utcnow()).days
        if days <= crit:
            problems.append(("P0", "%s 证书 %s 天后过期!" % (d, days)))
        elif days <= warn:
            problems.append(("P1", "%s 证书 %s 天后过期" % (d, days)))
    return problems


def _recent_logs(cfg, service):
    stack = cfg.get("STACK_NAME", "combee")
    win = int(cfg.get("LOG_WINDOW_MIN", 5))
    since = datetime.utcnow().timestamp() - win * 60
    since_str = datetime.utcfromtimestamp(since).strftime("%Y-%m-%dT%H:%M")
    return stack, service, win, since_str, since


def _container_log_paths(cfg, service):
    """返回该服务当前运行副本的宿主日志文件路径(Docker json-file,持久化落盘)。"""
    stack = cfg.get("STACK_NAME", "combee")
    paths = []
    cmd = "docker ps --no-trunc --filter name=%s_%s --format '{{.ID}}'" % (stack, service)
    for cid in run(cmd).split():
        p = "/var/lib/docker/containers/%s/%s-json.log" % (cid, cid)
        if os.path.exists(p):
            paths.append(p)
    return paths


def _raw_logs(cfg, service):
    """取指定服务窗口内的原始日志:直接读宿主机 /var/lib/docker/containers/*-json.log
    (持久化,滚动更新/容器重建后仍可读;由 logrotate 滚动保留最近 7 天)。
    json-file 每行: {"log":"<内层 JSON>","time":"2026-08-10T15:57:40.123Z",...}"""
    stack, svc, win, since_str, since_ts = _recent_logs(cfg, service)
    out = []
    for path in _container_log_paths(cfg, service):
        try:
            with open(path, "r", errors="replace") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        frame = json.loads(line)
                    except Exception:
                        continue
                    # 时间过滤:窗口外跳过
                    t = frame.get("time", "") or ""
                    if t:
                        ts = None
                        for fmt in ("%Y-%m-%dT%H:%M:%S.%fZ", "%Y-%m-%dT%H:%M:%SZ"):
                            try:
                                ts = datetime.strptime(t, fmt).timestamp()
                                break
                            except ValueError:
                                continue
                        if ts is not None and ts < since_ts:
                            continue
                    log = (frame.get("log", "") or "").strip()
                    if log.startswith("{"):
                        try:
                            obj = json.loads(log)
                            if isinstance(obj, dict):
                                out.append(obj)
                        except Exception:
                            pass
        except OSError:
            continue
    return out


def _err_fields(row):
    """从 tracing JSON 行提取 (message, operation, status, target);兼容字段在顶层或 fields 下。"""
    fields = row.get("fields", {}) if isinstance(row.get("fields"), dict) else {}
    message = fields.get("message") or row.get("message") or ""
    operation = fields.get("operation") or fields.get("event") or "-"
    status = fields.get("status") or row.get("status")
    target = row.get("target") or "-"
    # tower on_failure 层没有 operation;用 classification(如 "Status code: 500")补消息
    if operation == "-":
        classification = fields.get("classification") or ""
        if classification:
            message = classification if not message or message == "response failed" else message
        if target == "tower_http::trace::on_failure" and not message:
            message = "response failed (tower on_failure layer; see request.failed logs for operation)"
    return message, operation, status, target


def _sample_errs(errs, n=5):
    """采样:优先带 operation 的具体错误(tower on_failure 冗余层排后)。"""
    def rank(r):
        _, op, _, target = _err_fields(r)
        return (target == "tower_http::trace::on_failure", op == "-")
    return sorted(errs, key=rank)[:n]


def _grep_logs(cfg, service, pattern):
    n = 0
    for row in _raw_logs(cfg, service):
        if re.search(pattern, json.dumps(row, ensure_ascii=False)):
            n += 1
    return n


def error_rate(cfg):
    stack, svc, win, since_str, _since_ts = _recent_logs(cfg, "api-server")
    out = run(
        "docker service logs --since %s %s_api-server --no-trunc 2>&1 "
        "| grep -oE '\"status\":[0-9]+' | grep -oE '[0-9]+'" % (since_str, stack)
    )
    codes = [int(x) for x in out.split() if x.isdigit()]
    if not codes:
        return None
    fivexx = sum(1 for c in codes if c >= 500)
    rate = fivexx / len(codes) * 100
    p0 = int(cfg.get("ERR5_P0", 10))
    p1 = int(cfg.get("ERR5_P1", 2))
    samples = ""
    if fivexx > 0:
        rows = _raw_logs(cfg, "api-server")
        errs = [r for r in rows if _err_fields(r)[2] is not None and int(str(_err_fields(r)[2]).lstrip("-")) >= 500][:3]
        if errs:
            lines = []
            for r in errs:
                message, operation, status, target = _err_fields(r)
                lines.append("- %s: %s (status=%s)" % (operation, message[:120], status))
            samples = "\n" + "\n".join(lines)
    if rate >= p0:
        return ("P0", "5xx 率 %.1f%%(%s/%s 请求,最近 %s 分钟)%s" % (rate, fivexx, len(codes), win, samples))
    if rate >= p1:
        return ("P1", "5xx 率 %.1f%%(%s/%s 请求,最近 %s 分钟)%s" % (rate, fivexx, len(codes), win, samples))
    return None


def api_health(cfg):
    domains = cfg.get("DOMAINS", "").split()
    down, err5 = [], []
    for d in domains:
        code = run("curl -sk -o /dev/null -w '%%{http_code}' -m 8 https://%s/ 2>/dev/null" % d).strip()
        if not code or code == "000":
            down.append("- %s: 不可达(连接失败/超时)" % d)
        elif code.startswith("5"):
            err5.append("- %s: HTTP %s" % (d, code))
    if down:
        return ("P0", "\n".join(down))
    if err5:
        return ("P1", "\n".join(err5))
    return None


def error_volume(cfg):
    win = int(cfg.get("LOG_WINDOW_MIN", 5))
    rows = _raw_logs(cfg, "api-server")
    errs = [r for r in rows if str(r.get("level", "")).upper() == "ERROR"]
    if len(errs) >= 5:
        samples = []
        for r in _sample_errs(errs):
            message, operation, status, target = _err_fields(r)
            samples.append("- %s: %s (status=%s, %s)" % (operation, message[:140], status, target))
        return ("P1", "最近 %s 分钟 %s 条 ERROR,样本:\n%s" % (win, len(errs), "\n".join(samples)))
    return None


def auth_failures(cfg):
    win = int(cfg.get("LOG_WINDOW_MIN", 5))
    rows = _raw_logs(cfg, "api-server")
    bad = []
    for r in rows:
        f = r.get("fields", {}) if isinstance(r.get("fields"), dict) else {}
        status = f.get("status") or r.get("status")
        code = f.get("error_code") or r.get("error_code") or ""
        msg = f.get("message") or r.get("message") or ""
        if status == 401 or str(code).upper() in ("UNAUTHORIZED", "AUTH_FAILED") \
                or "unauthorized" in str(msg).lower():
            bad.append(r)
    if len(bad) >= 20:
        samples = []
        for r in _sample_errs(bad):
            message, operation, status, target = _err_fields(r)
            samples.append("- %s: %s (status=%s, %s)" % (operation, message[:140], status, target))
        return ("P1", "最近 %s 分钟 %s 次认证失败(401),样本:\n%s" % (win, len(bad), "\n".join(samples)))
    return None


def backup_failures(cfg):
    """备份失败:连续 >=3 次 → P1;1-2 次 → P2(提醒群)。"""
    n = _grep_logs(cfg, "data-node", 'backup.*fail|backup.*error')
    if n >= 3:
        return ("P1", "最近 5 分钟 %s 次备份失败(backup failed/error)" % n)
    if n >= 1:
        return ("P2", "最近 5 分钟 %s 次备份失败(backup failed/error)" % n)
    return None


def readonly_cells(cfg):
    """Cell 进入只读保护(完整性校验失败):数据完整性事件,任何一次都 P0。"""
    n = _grep_logs(cfg, "data-node", 'cell.readonly')
    if n >= 1:
        return ("P0", "最近 5 分钟 %s 个 Cell 进入只读保护(cell.readonly,integrity check 失败)" % n)
    return None


def failover_events(cfg):
    """Failover 触发或失败:手动/自动 failover 都是控制面关键事件,任何一次都 P1。"""
    n = _grep_logs(cfg, "api-server", 'failover')
    if n >= 1:
        return ("P1", "最近 5 分钟 %s 条 failover 事件(failover triggered/failed)" % n)
    return None


def background_job_failures(cfg):
    """usage flush / credits settlement 持续失败:计费链路受损,任何失败都 P1。"""
    n = _grep_logs(cfg, "api-server", 'usage flush failed|credits.settlement.failed')
    if n >= 1:
        return ("P1", "最近 5 分钟 %s 次后台任务失败(usage flush / credits settlement)" % n)
    return None


def slow_requests(cfg):
    n = _grep_logs(cfg, "api-server", 'request.slow')
    if n >= 5:
        return ("P2", "最近 5 分钟 %s 个慢请求(>500ms,request.slow)" % n)
    return None


def resource_watermark(cfg):
    warn = int(cfg.get("RES_WARN", 85))
    out = run("docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemPerc}}' | grep combee_")
    high = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 3:
            continue
        name, cpu, mem = parts[0], parts[1].rstrip("%"), parts[2].rstrip("%")
        try:
            if float(cpu) > warn or float(mem) > warn:
                high.append("- %s: CPU %s%% 内存 %s%%" % (name, cpu, mem))
        except ValueError:
            pass
    return ("P2", "\n".join(high)) if high else None


def main():
    cfg = load_config()
    dry = "--dry-run" in sys.argv
    state = load_state()
    now = time.time()

    def handle(key, level, title, content, webhook_key, result):
        """result: (level,msg) 或 None。触发→发告警;恢复→发已恢复。"""
        webhook = cfg.get(webhook_key, "")
        secret = cfg.get(webhook_key.replace("WEBHOOK", "SECRET"), "") or None
        was_active = state.get(key, {}).get("active", False)
        if result:
            if cooled(cfg, state, key) and was_active and not dry:
                print("[cooldown] %s 仍在告警中,跳过重复" % title)
                return
            lvl, msg = result
            text = "[Combee][%s] %s\n%s" % (lvl, title, msg)
            if dry:
                print("[dry-run] %s" % text)
                return
            if send(webhook, "%s" % title, "%s\n%s" % (msg, ""), lvl, secret):
                state[key] = {"last": now, "active": True}
                save_state(state)
        else:
            if was_active and not dry:
                if send(webhook, "已恢复: %s" % title, "✅ 告警已消除", "P2", secret):
                    state[key] = {"last": 0, "active": False}
                    save_state(state)
                    print("[recovery] %s" % title)
            elif dry and was_active:
                print("[dry-run] 恢复: %s" % title)

    # P0/P1 -> 运维告警群
    handle("svc", None, "服务副本不足", None, "FEISHU_ALERT_WEBHOOK", service_health(cfg))
    handle("disk", None, "磁盘预警", None, "FEISHU_ALERT_WEBHOOK", disk_usage(cfg))
    certs = cert_expiry(cfg)
    for lvl, msg in certs:
        handle("cert-" + msg[:24], lvl, "证书预警", msg, "FEISHU_ALERT_WEBHOOK", (lvl, msg))
    handle("err5", None, "5xx 错误率", None, "FEISHU_ALERT_WEBHOOK", error_rate(cfg))
    handle("apih", None, "API 可用性", None, "FEISHU_ALERT_WEBHOOK", api_health(cfg))
    handle("readonly", None, "Cell 只读保护(数据完整性)", None, "FEISHU_ALERT_WEBHOOK", readonly_cells(cfg))
    handle("failover", None, "Failover 事件", None, "FEISHU_ALERT_WEBHOOK", failover_events(cfg))
    handle("bgjob", None, "后台任务失败(usage/settlement)", None, "FEISHU_ALERT_WEBHOOK", background_job_failures(cfg))
    handle("errvol", None, "服务错误量", None, "FEISHU_ALERT_WEBHOOK", error_volume(cfg))
    handle("auth", None, "认证失败突增", None, "FEISHU_ALERT_WEBHOOK", auth_failures(cfg))
    bf = backup_failures(cfg)
    if bf and bf[0] == "P1":
        handle("backup", None, "备份失败", None, "FEISHU_ALERT_WEBHOOK", bf)
    elif bf:
        handle("backup-w", None, "备份失败(低优)", None, "FEISHU_WARN_WEBHOOK", bf)

    # P2 -> 提醒群
    handle("res", None, "资源水位", None, "FEISHU_WARN_WEBHOOK", resource_watermark(cfg))
    handle("slow", None, "慢请求", None, "FEISHU_WARN_WEBHOOK", slow_requests(cfg))

    if dry:
        print("[dry-run] 完成(未发送任何消息)")


if __name__ == "__main__":
    main()
