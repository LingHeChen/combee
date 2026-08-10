//! 健康检查辅助:进程内用纯 std 的 TCP 探活,供容器 HEALTHCHECK 使用。
//! 用法:`combee-api-server --healthcheck`(或 data-node),按探活结果 exit 0/1。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// 向 127.0.0.1:port 发一个 HTTP GET path,收到 2xx 返回 true。
/// 纯 std 实现,不依赖 curl/wget/reqwest,便于在 slim 容器里做 healthcheck。
pub fn tcp_http_get(port: u16, path: &str) -> bool {
    let addr = format!("127.0.0.1:{port}");
    let Ok(mut stream) = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return false,
    };
    let head = String::from_utf8_lossy(&buf[..n]);
    head.starts_with("HTTP/1.1 2") || head.starts_with("HTTP/1.0 2")
}

/// 从 main 调用:解析参数,若含 --healthcheck 则探活并按结果退出进程。
/// 返回是否"是 healthcheck 模式"(调用方应直接 exit)。
pub fn run_if_healthcheck(port: u16, path: &str) -> bool {
    if std::env::args().any(|a| a == "--healthcheck") {
        std::process::exit(if tcp_http_get(port, path) { 0 } else { 1 });
    }
    false
}
