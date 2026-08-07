//! Data Node 注册表:register / heartbeat / 健康过滤 / placement 选择。
//!
//! 运行在 API Server 进程内(设计文档 §23/§24 的 node registration / heartbeat /
//! placement 最小闭环):Data Node 启动时注册,周期心跳;心跳超时视为不可用;
//! 创建数据库时用 round-robin 从健康节点中选择放置位置。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use combee_common::NodeId;
use serde::Serialize;

/// 心跳超时:超过该时间未心跳视为不可用(placement 时跳过)。
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
struct NodeEntry {
    addr: String,
    capacity: usize,
    active_conns: usize,
    last_heartbeat: Instant,
    registered_at: Instant,
}

/// 节点状态快照(metrics / `/internal/nodes`)。
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub id: NodeId,
    pub addr: String,
    pub capacity: usize,
    pub active_conns: usize,
    pub healthy: bool,
    /// 注册时长(秒)。
    pub age_secs: u64,
}

#[derive(Clone, Default)]
pub struct NodeRegistry {
    inner: Arc<RwLock<HashMap<NodeId, NodeEntry>>>,
    /// round-robin 游标。
    rr: Arc<AtomicU64>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个节点,返回分配的 ID。
    pub fn register(&self, addr: String, capacity: usize) -> NodeId {
        self.register_with_id(None, addr, capacity)
    }

    /// 注册一个节点(可指定 ID,重启保持身份)。ID 已存在时更新地址与心跳。
    pub fn register_with_id(&self, id: Option<NodeId>, addr: String, capacity: usize) -> NodeId {
        let id = id.unwrap_or_default();
        let now = Instant::now();
        let mut g = self.inner.write().unwrap();
        match g.get_mut(&id) {
            Some(e) => {
                e.addr = addr;
                e.capacity = capacity;
                e.last_heartbeat = now;
            }
            None => {
                g.insert(
                    id,
                    NodeEntry {
                        addr,
                        capacity,
                        active_conns: 0,
                        last_heartbeat: now,
                        registered_at: now,
                    },
                );
            }
        }
        id
    }

    /// 上报心跳。返回节点是否已知。
    pub fn heartbeat(&self, id: NodeId, active_conns: usize) -> bool {
        let mut g = self.inner.write().unwrap();
        match g.get_mut(&id) {
            Some(e) => {
                e.active_conns = active_conns;
                e.last_heartbeat = Instant::now();
                true
            }
            None => false,
        }
    }

    /// 注销节点(Data Node 退出时调用)。返回是否存在。
    pub fn unregister(&self, id: NodeId) -> bool {
        self.inner.write().unwrap().remove(&id).is_some()
    }

    fn entry_healthy(entry: &NodeEntry) -> bool {
        entry.last_heartbeat.elapsed() < HEARTBEAT_TIMEOUT
    }

    /// 节点是否健康(心跳未超时)。
    pub fn is_healthy(&self, id: NodeId) -> bool {
        let g = self.inner.read().unwrap();
        g.get(&id).is_some_and(Self::entry_healthy)
    }

    /// 健康节点的 RPC 地址;节点缺失或心跳超时返回 None。
    pub fn addr(&self, id: NodeId) -> Option<String> {
        let g = self.inner.read().unwrap();
        g.get(&id)
            .filter(|e| Self::entry_healthy(e))
            .map(|e| e.addr.clone())
    }

    /// 健康节点列表 (id, addr)。
    pub fn healthy(&self) -> Vec<(NodeId, String)> {
        let g = self.inner.read().unwrap();
        g.iter()
            .filter(|(_, e)| Self::entry_healthy(e))
            .map(|(id, e)| (*id, e.addr.clone()))
            .collect()
    }

    /// round-robin 选一个健康节点(数据库 placement)。
    pub fn pick(&self) -> Option<(NodeId, String)> {
        let healthy = self.healthy();
        if healthy.is_empty() {
            return None;
        }
        let idx = (self.rr.fetch_add(1, Ordering::Relaxed) % healthy.len() as u64) as usize;
        healthy.get(idx).cloned()
    }

    /// 全部节点状态(metrics)。
    pub fn list(&self) -> Vec<NodeInfo> {
        let g = self.inner.read().unwrap();
        g.iter()
            .map(|(id, e)| NodeInfo {
                id: *id,
                addr: e.addr.clone(),
                capacity: e.capacity,
                active_conns: e.active_conns,
                healthy: Self::entry_healthy(e),
                age_secs: e.registered_at.elapsed().as_secs(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_heartbeat_pick_and_unregister() {
        let reg = NodeRegistry::new();
        let a = reg.register("http://a:9000".into(), 10);
        let b = reg.register("http://b:9000".into(), 10);

        // 心跳
        assert!(reg.heartbeat(a, 3));
        assert!(!reg.heartbeat(NodeId::new(), 0), "unknown node");

        // 健康列表
        assert_eq!(reg.healthy().len(), 2);
        assert!(reg.is_healthy(a));

        // round-robin:两个节点交替
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4 {
            let (id, _) = reg.pick().unwrap();
            seen.insert(id);
        }
        assert_eq!(seen.len(), 2, "round-robin should cover both nodes");

        // 注销后只剩一个
        assert!(reg.unregister(b));
        assert_eq!(reg.healthy().len(), 1);
        let (id, _) = reg.pick().unwrap();
        assert_eq!(id, a);
    }

    #[test]
    fn stale_heartbeat_marks_node_unhealthy() {
        let reg = NodeRegistry::new();
        let id = reg.register("http://a:9000".into(), 10);
        assert!(reg.is_healthy(id));

        // 模拟心跳超时:直接改 entry 的 last_heartbeat
        {
            let mut g = reg.inner.write().unwrap();
            let e = g.get_mut(&id).unwrap();
            e.last_heartbeat = Instant::now() - HEARTBEAT_TIMEOUT - Duration::from_secs(1);
        }
        assert!(!reg.is_healthy(id));
        assert!(reg.addr(id).is_none(), "stale node must not be routable");
        assert!(reg.pick().is_none(), "no healthy node to place");
    }
}
