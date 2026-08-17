//! Data Node 注册表:register / heartbeat / 健康过滤 / placement 选择。
//!
//! 两种模式:
//! - **内存模式**(dev/单进程):`NodeRegistry::new()`,注册表在进程内存;
//! - **PG 模式**(多 API 副本):`NodeRegistry::with_pg(metadata)`,PostgreSQL 是 authority,
//!   本地内存是 TTL 缓存(默认 3s)。任意 API 副本都能看到同一份节点状态。
//!
//! 设计(shared authority + eventual local cache):
//! - register / heartbeat / unregister 写 PostgreSQL(低频,直接落库);
//! - addr / healthy / pick 读本地缓存,缓存过期时全量刷新自 PG;
//! - 不做 LISTEN/NOTIFY,TTL 兜底足够(3s 内收敛)。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use combee_common::NodeId;
use combee_metadata::MetadataStore;
use serde::Serialize;

/// 心跳超时:超过该时间未心跳视为不可用(placement 时跳过)。
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
/// PG 模式下本地缓存的有效期。
pub const PG_CACHE_TTL: Duration = Duration::from_secs(3);

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

#[derive(Clone)]
pub struct NodeRegistry {
    inner: Arc<RwLock<HashMap<NodeId, NodeEntry>>>,
    /// round-robin 游标。
    rr: Arc<AtomicU64>,
    /// PG 共享存储(Some = 多副本模式)。
    pg: Option<Arc<dyn MetadataStore>>,
    /// 本地缓存有效期(PG 模式)。
    cache_ttl: Duration,
    /// 上次全量刷新时间。
    last_refresh: Arc<RwLock<Instant>>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl NodeRegistry {
    /// 内存模式(dev / 单进程)。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            rr: Arc::new(AtomicU64::new(0)),
            pg: None,
            cache_ttl: PG_CACHE_TTL,
            last_refresh: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// PG 模式:PostgreSQL 为 authority,本地为 TTL 缓存。
    pub fn with_pg(metadata: Arc<dyn MetadataStore>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            rr: Arc::new(AtomicU64::new(0)),
            pg: Some(metadata),
            cache_ttl: PG_CACHE_TTL,
            last_refresh: Arc::new(RwLock::new(Instant::now() - PG_CACHE_TTL)),
        }
    }

    /// PG 模式下,若本地缓存过期则全量刷新(从 authority 拉取)。
    async fn ensure_fresh(&self) {
        let Some(pg) = &self.pg else { return };
        let stale = self.last_refresh.read().unwrap().elapsed() >= self.cache_ttl;
        if !stale {
            return;
        }
        match pg.list_data_nodes().await {
            Ok(records) => {
                let now_unix = unix_now();
                let mut g = self.inner.write().unwrap();
                g.clear();
                for rec in records {
                    let age = now_unix.saturating_sub(rec.last_heartbeat_at);
                    g.insert(
                        rec.id,
                        NodeEntry {
                            addr: rec.addr,
                            capacity: rec.capacity,
                            active_conns: rec.active_conns,
                            last_heartbeat: Instant::now() - Duration::from_secs(age),
                            registered_at: Instant::now()
                                - Duration::from_secs(now_unix.saturating_sub(rec.created_at)),
                        },
                    );
                }
                *self.last_refresh.write().unwrap() = Instant::now();
            }
            Err(e) => {
                tracing::warn!("refresh node registry from pg failed: {e}");
            }
        }
    }

    /// 注册一个节点,返回分配的 ID。
    pub async fn register(&self, addr: String, capacity: usize) -> NodeId {
        self.register_with_id(None, addr, capacity).await
    }

    /// 注册一个节点(可指定 ID,重启保持身份)。ID 已存在时更新地址与心跳。
    pub async fn register_with_id(
        &self,
        id: Option<NodeId>,
        addr: String,
        capacity: usize,
    ) -> NodeId {
        let id = id.unwrap_or_default();
        let now = Instant::now();
        {
            let mut g = self.inner.write().unwrap();
            match g.get_mut(&id) {
                Some(e) => {
                    e.addr = addr.clone();
                    e.capacity = capacity;
                    e.last_heartbeat = now;
                }
                None => {
                    g.insert(
                        id,
                        NodeEntry {
                            addr: addr.clone(),
                            capacity,
                            active_conns: 0,
                            last_heartbeat: now,
                            registered_at: now,
                        },
                    );
                }
            }
        }
        if let Some(pg) = &self.pg {
            let _ = pg.upsert_data_node(id, addr, capacity).await;
        }
        id
    }

    /// 上报心跳。返回节点是否已知。
    pub async fn heartbeat(&self, id: NodeId, active_conns: usize) -> bool {
        let known = {
            let mut g = self.inner.write().unwrap();
            match g.get_mut(&id) {
                Some(e) => {
                    e.active_conns = active_conns;
                    e.last_heartbeat = Instant::now();
                    true
                }
                None => false,
            }
        };
        if known {
            if let Some(pg) = &self.pg {
                let _ = pg.heartbeat_data_node(id, active_conns).await;
            }
        }
        known
    }

    /// 注销节点(Data Node 退出时调用)。返回是否存在。
    pub async fn unregister(&self, id: NodeId) -> bool {
        let existed = self.inner.write().unwrap().remove(&id).is_some();
        if existed {
            if let Some(pg) = &self.pg {
                let _ = pg.unregister_data_node(id).await;
            }
        }
        existed
    }

    fn entry_healthy(entry: &NodeEntry) -> bool {
        entry.last_heartbeat.elapsed() < HEARTBEAT_TIMEOUT
    }

    /// 节点是否健康(心跳未超时)。
    pub async fn is_healthy(&self, id: NodeId) -> bool {
        self.ensure_fresh().await;
        let g = self.inner.read().unwrap();
        g.get(&id).is_some_and(Self::entry_healthy)
    }

    /// 健康节点的 RPC 地址;节点缺失或心跳超时返回 None。
    pub async fn addr(&self, id: NodeId) -> Option<String> {
        self.ensure_fresh().await;
        let g = self.inner.read().unwrap();
        g.get(&id)
            .filter(|e| Self::entry_healthy(e))
            .map(|e| e.addr.clone())
    }

    /// 健康节点列表 (id, addr)。
    pub async fn healthy(&self) -> Vec<(NodeId, String)> {
        self.ensure_fresh().await;
        let g = self.inner.read().unwrap();
        g.iter()
            .filter(|(_, e)| Self::entry_healthy(e))
            .map(|(id, e)| (*id, e.addr.clone()))
            .collect()
    }

    /// round-robin 选一个健康节点(数据库 placement)。
    pub async fn pick(&self) -> Option<(NodeId, String)> {
        let healthy = self.healthy().await;
        if healthy.is_empty() {
            return None;
        }
        let idx = (self.rr.fetch_add(1, Ordering::Relaxed) % healthy.len() as u64) as usize;
        healthy.get(idx).cloned()
    }

    /// 全部节点状态(metrics)。
    pub async fn list(&self) -> Vec<NodeInfo> {
        self.ensure_fresh().await;
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

    /// PG 模式可用(供日志/决策)。
    pub fn shared(&self) -> bool {
        self.pg.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_heartbeat_pick_and_unregister() {
        let reg = NodeRegistry::new();
        let a = reg.register("http://a:9000".into(), 10).await;
        let _b = reg.register("http://b:9000".into(), 10).await;

        // 心跳
        assert!(reg.heartbeat(a, 3).await);
        assert!(!reg.heartbeat(NodeId::new(), 0).await, "unknown node");

        // 健康列表
        assert_eq!(reg.healthy().await.len(), 2);
        assert!(reg.is_healthy(a).await);

        // round-robin:两个节点交替
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4 {
            let (id, _) = reg.pick().await.unwrap();
            seen.insert(id);
        }
        assert_eq!(seen.len(), 2, "round-robin should cover both nodes");

        // 注销后只剩一个
        assert!(reg.unregister(a).await);
        assert_eq!(reg.healthy().await.len(), 1);
        assert!(!reg.unregister(a).await);
    }
}
