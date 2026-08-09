//! 控制面目录数据:记录每个 Cell 的归属与状态。
//!
//! 关键原则:Metadata 是目录,不是用户数据库 —— 这里绝不存放用户业务数据。
//! 默认使用进程内 [`InMemoryStore`](store::InMemoryStore),生产可用
//! PostgreSQL 后端 [`PostgresStore`](postgres::PostgresStore)(接口见 [`store::MetadataStore`])。

pub mod postgres;
pub mod store;

pub use postgres::PostgresStore;
pub use store::{
    ApiKeyRecord, DataNodeRecord, DEFAULT_TENANT, DatabaseRecord, DatabaseState, InMemoryStore,
    MetadataStore, TenantRecord, WaitlistEntry,
};
