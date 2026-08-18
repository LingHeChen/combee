//! 轻量 Prometheus 文本指标(无外部依赖)。
//!
//! 为 Cloud Alpha 提供最小可观测闭环:API Server 与 Data Node 各自暴露
//! `GET /metrics`,输出 Prometheus 文本格式,由托管监控/探针采集。
//! 遵循 `COMBEE_OBSERVABILITY_ALERTING_PLAN.md` §14 的最小指标集与 §15
//! 基数规则:标签只用低基数维度(service / operation / status_class / node_role /
//! error_class),禁止 tenant_id / cell_id / request_id 等作为标签。
//!
//! 实现说明:计数器/仪表/直方图全部落在进程内注册表,线程安全;
//! 每次变更短暂持有锁(纳秒~微秒级),对请求热路径开销可忽略。
//! 直方图为固定桶(延迟秒),输出 count/sum/分桶,可算 p50/p99。

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::OnceLock;

/// 延迟直方图固定桶(秒)。覆盖 1ms ~ 10s。
const DURATION_BUCKETS: &[f64] = &[
    0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

struct Histogram {
    buckets: Vec<u64>, // len == DURATION_BUCKETS.len()
    count: u64,
    sum: f64,
}

impl Histogram {
    fn observe(&mut self, v: f64) {
        self.count += 1;
        self.sum += v;
        if v.is_finite() {
            for (i, upper) in DURATION_BUCKETS.iter().enumerate() {
                if v <= *upper {
                    self.buckets[i] += 1;
                }
            }
        }
    }
}

struct Registry {
    counters: Mutex<BTreeMap<String, u64>>,
    gauges: Mutex<BTreeMap<String, i64>>,
    histograms: Mutex<BTreeMap<String, Histogram>>,
}

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| Registry {
        counters: Mutex::new(BTreeMap::new()),
        gauges: Mutex::new(BTreeMap::new()),
        histograms: Mutex::new(BTreeMap::new()),
    })
}

/// 每个 map 的 series 数上限:防高基数 label(如把 KV key 当 label)无限增长撑爆内存。
/// 超限后不再新增 series(已有 series 继续更新);正常低基数指标远达不到此值。
const MAX_SERIES: usize = 5000;

/// 把 name + 有序标签编码为内部键(`name{l="v",...}`)。
fn key(name: &str, labels: &[(&str, &str)]) -> String {
    if labels.is_empty() {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len() + 16);
    out.push_str(name);
    out.push('{');
    for (i, (k, v)) in labels.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&v.replace('\\', "\\\\").replace('"', "\\\""));
        out.push('"');
    }
    out.push('}');
    out
}

/// 计数器 +1。
pub fn counter_inc(name: &str, labels: &[(&str, &str)]) {
    counter_add(name, labels, 1);
}

/// 计数器 +delta(≥0)。
pub fn counter_add(name: &str, labels: &[(&str, &str)], delta: u64) {
    if delta == 0 {
        return;
    }
    let k = key(name, labels);
    let mut c = registry().counters.lock().unwrap();
    if let Some(v) = c.get_mut(&k) {
        *v += delta;
    } else if c.len() < MAX_SERIES {
        c.insert(k, delta);
    }
}

/// 仪表:设置当前值(连接数、lag 等)。
pub fn gauge_set(name: &str, labels: &[(&str, &str)], v: i64) {
    let k = key(name, labels);
    let mut g = registry().gauges.lock().unwrap();
    if g.contains_key(&k) || g.len() < MAX_SERIES {
        g.insert(k, v);
    }
}

/// 直方图:记录一次观测(延迟秒等)。
pub fn histogram_observe(name: &str, labels: &[(&str, &str)], v: f64) {
    let k = key(name, labels);
    let mut h = registry().histograms.lock().unwrap();
    if let Some(hist) = h.get_mut(&k) {
        hist.observe(v);
    } else if h.len() < MAX_SERIES {
        h.entry(k)
            .or_insert_with(|| Histogram {
                buckets: vec![0; DURATION_BUCKETS.len()],
                count: 0,
                sum: 0.0,
            })
            .observe(v);
    }
}

fn escape_meta(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn render_map(out: &mut String, mtype: &str, help: &str, map: &BTreeMap<String, u64>) {
    out.push_str("# HELP ");
    out.push_str(&help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(mtype);
    out.push('\n');
    for (k, v) in map {
        out.push_str(k);
        out.push(' ');
        out.push_str(&v.to_string());
        out.push('\n');
    }
}

/// 渲染 Prometheus 文本格式快照。
pub fn render() -> String {
    let r = registry();
    let mut out = String::with_capacity(4096);

    {
        let counters = r.counters.lock().unwrap();
        render_map(&mut out, "counter", "combee counters", &counters);
    }
    {
        let gauges = r.gauges.lock().unwrap();
        out.push_str("# HELP combee_gauges combee gauges\n# TYPE combee_gauges gauge\n");
        for (k, v) in gauges.iter() {
            out.push_str(k);
            out.push(' ');
            out.push_str(&v.to_string());
            out.push('\n');
        }
    }
    {
        let histograms = r.histograms.lock().unwrap();
        // 直方图:输出 _bucket/_count/_sum 三组。name 已含标签。
        for (name, h) in histograms.iter() {
            let base = name; // 如 combee_request_duration_seconds{op="..."}
            for (i, upper) in DURATION_BUCKETS.iter().enumerate() {
                out.push_str(base);
                out.push_str("_bucket{le=\"");
                out.push_str(&upper.to_string());
                out.push_str("\"} ");
                out.push_str(&h.buckets[i].to_string());
                out.push('\n');
            }
            out.push_str(base);
            out.push_str("_bucket{le=\"+Inf\"} ");
            out.push_str(&h.count.to_string());
            out.push('\n');
            out.push_str(base);
            out.push_str("_count ");
            out.push_str(&h.count.to_string());
            out.push('\n');
            out.push_str(base);
            out.push_str("_sum ");
            out.push_str(&format_sum(h.sum));
            out.push('\n');
        }
    }
    let _ = escape_meta; // 保留 helper,便于未来扩展(标签值转义已在 key() 处理)
    out
}

fn format_sum(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_inc_and_render() {
        // 先清空再测(全局注册表,避免跨用例污染)
        let mut c = registry().counters.lock().unwrap();
        c.clear();
        drop(c);
        counter_inc("combee_http_requests_total", &[("op", "sql")]);
        counter_inc("combee_http_requests_total", &[("op", "sql")]);
        counter_inc("combee_http_requests_total", &[("op", "kv")]);
        let s = render();
        assert!(
            s.contains("combee_http_requests_total{op=\"sql\"} 2"),
            "{s}"
        );
        assert!(s.contains("combee_http_requests_total{op=\"kv\"} 1"));
    }

    #[test]
    fn histogram_renders_buckets_count_sum() {
        let mut h = registry().histograms.lock().unwrap();
        h.clear();
        drop(h);
        histogram_observe("combee_request_duration_seconds", &[("op", "sql")], 0.005);
        histogram_observe("combee_request_duration_seconds", &[("op", "sql")], 2.0);
        let s = render();
        assert!(s.contains("combee_request_duration_seconds{op=\"sql\"}_bucket{le=\"0.005\"} 1"));
        assert!(s.contains("combee_request_duration_seconds{op=\"sql\"}_bucket{le=\"2.5\"} 2"));
        assert!(s.contains("combee_request_duration_seconds{op=\"sql\"}_count 2"));
        assert!(s.contains("combee_request_duration_seconds{op=\"sql\"}_sum 2.005"));
    }

    #[test]
    fn gauge_set_and_render() {
        let mut g = registry().gauges.lock().unwrap();
        g.clear();
        drop(g);
        gauge_set(
            "combee_open_sqlite_connections",
            &[("node_role", "data")],
            3,
        );
        let s = render();
        assert!(s.contains("combee_open_sqlite_connections{node_role=\"data\"} 3"));
    }

    #[test]
    fn label_escaping() {
        let mut c = registry().counters.lock().unwrap();
        c.clear();
        drop(c);
        counter_inc("x", &[("op", "a\"b\\c")]);
        let s = render();
        assert!(s.contains("x{op=\"a\\\"b\\\\c\"} 1"), "{s}");
    }
}
