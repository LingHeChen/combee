#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""飞书 webhook 发送器(告警/提醒/日报共用)。
用法:send_feishu.py <webhook_url> <title> <content> [--level P0|P1|P2]
内容为 markdown 文本;P0/P1 会带醒目标签。
"""
import base64
import hashlib
import hmac
import json
import sys
import time
import urllib.parse
import urllib.request

TIMEOUT = 10


def _signed_url(webhook: str, secret: str) -> str:
    """飞书自定义机器人加签:URL 追加 timestamp + sign(HMAC-SHA256)。"""
    ts = str(int(time.time()))
    # 飞书签名算法:key = timestamp+"\n"+secret,msg 为空
    string_to_sign = ts + "\n" + secret
    digest = hmac.new(string_to_sign.encode(), b"", hashlib.sha256).digest()
    sign = urllib.parse.quote(base64.b64encode(digest))
    sep = "&" if "?" in webhook else "?"
    return webhook + sep + "timestamp=" + ts + "&sign=" + sign


def send(webhook: str, title: str, content: str, level: str = "P2", secret=None) -> bool:  # secret: Optional[str]
    if not webhook or webhook == "填写你的webhook":
        print("[skip] webhook 未配置,消息未发送:", title, file=sys.stderr)
        return False
    # 固定关键字:飞书机器人安全设置需配置自定义关键词 "Combee",消息必须包含它才会发送。
    tag = f"[Combee][{level}] {title}"
    md = f"**{tag}**\n{content}"
    payload = {
        "msg_type": "interactive",
        "card": {
            "header": {
                "title": {"tag": "plain_text", "content": tag},
                "template": {"P0": "red", "P1": "orange", "P2": "blue"}.get(level, "blue"),
            },
            "elements": [{"tag": "markdown", "content": content}],
        },
    }
    url = _signed_url(webhook, secret) if secret else webhook
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            body = json.loads(resp.read())
            if body.get("code") != 0 and body.get("StatusCode") != 0:
                print(f"[error] 飞书返回: {body}", file=sys.stderr)
                return False
            return True
    except Exception as e:
        print(f"[error] 飞书发送失败: {e}", file=sys.stderr)
        return False


if __name__ == "__main__":
    if len(sys.argv) < 4:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    webhook, title, content = sys.argv[1], sys.argv[2], sys.argv[3]
    level = sys.argv[4] if len(sys.argv) > 4 else "P2"
    sys.exit(0 if send(webhook, title, content, level) else 1)
