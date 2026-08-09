//! Release Gate 集成测试入口(对应 artifacts/engineering/COMBEE_RELEASE_READINESS_TEST_PLAN.md)。
//! 子模块位于 tests/release/*.rs。
#![allow(clippy::duplicate_mod)]

#[path = "release/backup_restore.rs"]
mod backup_restore;
#[path = "release/fuzz.rs"]
mod fuzz;
#[path = "release/golden_path.rs"]
mod golden_path;
#[path = "release/isolation.rs"]
mod isolation;
#[path = "release/resource.rs"]
mod resource;
