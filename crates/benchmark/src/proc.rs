//! 进程资源度量:容量基准需要 RSS / CPU / fd 数据。
//!
//! Linux 上通过 `/proc/self/*` 读取(容器内可用);其他平台返回 `None` 兜底
//! (资源列在非 Linux 下显示为 "-")。

/// 一次资源快照。
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcStats {
    /// 常驻内存(kB)。
    pub rss_kb: Option<u64>,
    /// 进程 CPU 时间(utime + stime,单位 USER_HZ 时钟 tick,Linux 通常 100/s)。
    pub cpu_ticks: Option<u64>,
    /// 打开的文件描述符数量。
    pub fd_count: Option<u64>,
}

pub fn read_stats() -> ProcStats {
    ProcStats {
        rss_kb: read_vmrss(),
        cpu_ticks: read_cpu_ticks(),
        fd_count: read_fd_count(),
    }
}

/// 由两次快照的 tick 差与墙钟时间估算 CPU 使用率(%)。
/// 1 tick = 1/100 s,因此 `Δticks / Δwall_secs` 即 CPU 百分比。
pub fn cpu_percent(before: &ProcStats, after: &ProcStats, wall_secs: f64) -> Option<f64> {
    let (b, a) = (before.cpu_ticks?, after.cpu_ticks?);
    if wall_secs <= 0.0 {
        return None;
    }
    let dt = a.saturating_sub(b) as f64;
    Some(dt / wall_secs)
}

#[cfg(target_os = "linux")]
fn read_vmrss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn read_vmrss() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn read_cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // comm(字段 2)在括号内且可能含空格:从最后一个 ')' 之后开始解析
    let rest = stat.rfind(')')?;
    let fields: Vec<&str> = stat[rest + 2..].split_whitespace().collect();
    // rest[0] 对应字段 3(state),因此 utime(14) = rest[11],stime(15) = rest[12]
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

#[cfg(not(target_os = "linux"))]
fn read_cpu_ticks() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn read_fd_count() -> Option<u64> {
    let n = std::fs::read_dir("/proc/self/fd").ok()?.count();
    Some(n as u64)
}

#[cfg(not(target_os = "linux"))]
fn read_fd_count() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_are_readable_on_linux() {
        let s = read_stats();
        if cfg!(target_os = "linux") {
            assert!(s.rss_kb.is_some_and(|v| v > 0), "VmRSS should be readable");
            assert!(s.cpu_ticks.is_some(), "utime+stime should be readable");
            assert!(
                s.fd_count.is_some_and(|v| v > 0),
                "at least stdin/out/err fds"
            );
        } else {
            assert!(s.rss_kb.is_none(), "non-linux falls back to None");
        }
    }

    #[test]
    fn cpu_percent_is_bounded() {
        let before = ProcStats {
            cpu_ticks: Some(1000),
            ..Default::default()
        };
        let after = ProcStats {
            cpu_ticks: Some(1200),
            ..Default::default()
        };
        // 200 ticks / 2s = 100%
        let pct = cpu_percent(&before, &after, 2.0).unwrap();
        assert!((pct - 100.0).abs() < 1e-9);
        // 回退:同样增量但墙钟更长 → 更低
        let pct2 = cpu_percent(&before, &after, 4.0).unwrap();
        assert!((pct2 - 50.0).abs() < 1e-9);
        // 快照缺失 → None
        assert!(cpu_percent(&ProcStats::default(), &after, 1.0).is_none());
    }
}
