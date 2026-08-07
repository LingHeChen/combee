//! Data Node 节点代理:向 API Server 注册 + 周期心跳(自愈)。
//!
//! 与启动顺序无关:每 2s 一次 tick —— 尚未注册则尝试注册,已注册则上报心跳;
//! API Server 尚未就绪时注册失败会自动重试。退出时注销。

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use combee_common::NodeId;
use combee_common::rpc::{NodeRegisterRequest, NodeRegisterResponse, NodeUnregisterRequest};

/// 心跳/重试间隔。
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Data Node 节点代理。
pub struct NodeAgent {
    api_url: String,
    advertise_url: String,
    capacity: usize,
    http: reqwest::Client,
    id: RwLock<Option<NodeId>>,
    stopped: AtomicBool,
    /// 节点 ID 持久化文件(kill -9 重启后保持身份)。
    id_file: Option<std::path::PathBuf>,
}

impl NodeAgent {
    /// 启动节点代理:立即尝试注册,后台周期心跳/重试。
    /// 返回 (agent, 心跳任务句柄)。
    /// 启动节点代理。`id_file` 用于持久化节点 ID(重启保持身份;None 则每次新 ID)。
    pub async fn start(
        api_url: &str,
        advertise_url: &str,
        capacity: usize,
        id_file: Option<&std::path::Path>,
    ) -> (std::sync::Arc<Self>, tokio::task::JoinHandle<()>) {
        // 读取持久化 ID(重启后沿用,存量 Cell 路由不失效)
        let persisted = id_file
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| s.parse::<NodeId>().ok());
        let agent = std::sync::Arc::new(Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            advertise_url: advertise_url.to_string(),
            capacity,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client"),
            id: RwLock::new(persisted),
            stopped: AtomicBool::new(false),
            id_file: id_file.map(|p| p.to_path_buf()),
        });
        // 首次注册立即尝试(失败则由后台循环重试)
        agent.tick().await;
        let heartbeat = agent.clone().spawn_loop();
        (agent, heartbeat)
    }

    fn spawn_loop(self: std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if self.stopped.load(Ordering::Relaxed) {
                    return;
                }
                self.tick().await;
            }
        })
    }

    /// 单次 tick:注册(带本地持久化 id,幂等)并刷新心跳。
    /// API Server 重启后 registry 清空也能用同一 id 重新注册,身份不变。
    async fn tick(&self) {
        match self.register_once().await {
            Ok(id) => {
                if *self.id.read().unwrap() != Some(id) {
                    tracing::info!(node = %id, "registered with api server {}", self.api_url);
                }
                *self.id.write().unwrap() = Some(id);
            }
            Err(e) => {
                tracing::warn!("register with {} failed, will retry: {e}", self.api_url)
            }
        }
    }

    async fn register_once(&self) -> Result<NodeId, String> {
        let my_id = *self.id.read().unwrap();
        let resp = self
            .http
            .post(format!("{}/internal/nodes/register", self.api_url))
            .json(&NodeRegisterRequest {
                id: my_id,
                addr: self.advertise_url.clone(),
                capacity: self.capacity,
            })
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("register returned {}", resp.status()));
        }
        let r: NodeRegisterResponse = resp.json().await.map_err(|e| e.to_string())?;
        // 持久化 ID(首次注册后写入,重启沿用)
        #[allow(clippy::collapsible_if)]
        if let Some(path) = &self.id_file {
            if let Err(e) = std::fs::write(path, r.id.to_string()) {
                tracing::warn!(node = %r.id, "persist node id failed: {e}");
            }
        }
        Ok(r.id)
    }

    /// 注销(进程退出时调用)。
    pub async fn unregister(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        let Some(id) = *self.id.read().unwrap() else {
            return;
        };
        let resp = self
            .http
            .post(format!("{}/internal/nodes/unregister", self.api_url))
            .json(&NodeUnregisterRequest { id })
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                tracing::info!(node = %id, "unregistered from api server")
            }
            Ok(r) => tracing::warn!(node = %id, "unregister returned {}", r.status()),
            Err(e) => tracing::warn!(node = %id, "unregister failed: {e}"),
        }
    }

    /// 当前节点 ID(尚未注册成功时为 None)。
    pub fn id(&self) -> Option<NodeId> {
        *self.id.read().unwrap()
    }

    /// API Server base URL。
    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}
