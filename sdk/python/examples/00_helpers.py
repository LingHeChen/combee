"""示例共享:读取环境变量创建客户端。"""
import os
from combee import Combee


def client() -> Combee:
    return Combee(
        base_url=os.environ.get("COMBEE_URL", "http://127.0.0.1:8080"),
        api_key=os.environ.get("COMBEE_API_KEY", "dev-key"),
    )
