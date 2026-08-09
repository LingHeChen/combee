//! Combee V0 集成测试(HTTP 栈):database lifecycle、SQL、KV、TTL、
//! lazy create、active-db 上限、auth 与访问控制。
//!
//! 公共 helper 见 `tests/common/mod.rs`,每个测试的目的与预期结果见 `artifacts/engineering/TESTING.md`。

mod common;

use axum::http::{Method, StatusCode};
use combee_api_server::client::DataNodeClient;
use common::{create_db, send, test_app, test_app_with_keys};
use serde_json::json;

// ---- Database lifecycle ----

/// 目的:验证 create → list → sql → delete 的完整生命周期,以及
/// 删除后访问返回 404、重复删除返回 404。
#[tokio::test]
async fn database_lifecycle() {
    let (app, _, _dir) = test_app(16);

    // create → list
    let id = create_db(&app).await;
    let (status, body) = send(&app, Method::GET, "/v1/databases", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&id.as_str()));

    // 每次创建生成不同的 uuid
    let (status, body) = send(&app, Method::POST, "/v1/databases", Some(json!({})), None).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(body["id"].as_str().unwrap(), id.as_str());

    // 已创建 db 可执行 SQL
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "sql on created db: {body}");

    // delete → 204,再访问 404
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/v1/databases/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 重复删除 → 404
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/v1/databases/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- SQL ----

/// 目的:验证建表、参数绑定(位置参数)、带条件的查询结果,以及语法错误返回 400。
#[tokio::test]
async fn sql_basic_and_params() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create table: {body}");

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(
            json!({"sql": "INSERT INTO users (name, age) VALUES (?, ?)", "params": ["alice", 30]}),
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows_affected"], 1);

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT name, age FROM users WHERE age > ?", "params": [25]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["columns"], json!(["name", "age"]));
    assert_eq!(body["rows"], json!([["alice", 30]]));

    // 语法错误 → 400
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "THIS IS NOT SQL"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 目的:验证各类 JSON 参数类型到 SQLite 值的映射
/// (null / bool / 整数 / 浮点),以及 NULL 返回值的往返。
#[tokio::test]
async fn sql_value_types_roundtrip() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE vals (n INTEGER, r REAL, t TEXT, flag INTEGER)"})),
        None,
    )
    .await;

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "INSERT INTO vals VALUES (?, ?, ?, ?)", "params": [null, 3.5, "héllo", true]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows_affected"], 1);

    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT n, r, t, flag FROM vals"})),
        None,
    )
    .await;
    // bool true → INTEGER 1;null → null;real → 3.5
    assert_eq!(body["rows"], json!([[null, 3.5, "héllo", 1]]));

    // 浮点参数参与比较
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT t FROM vals WHERE r > ?", "params": [3.0]})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([["héllo"]]));
}

/// 目的:验证 UPDATE 影响多行时 rows_affected 统计正确。
#[tokio::test]
async fn sql_rows_affected_on_update() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE t (v INTEGER)"})),
        None,
    )
    .await;
    let (_, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": [
            {"sql": "INSERT INTO t VALUES (1)"},
            {"sql": "INSERT INTO t VALUES (2)"},
            {"sql": "INSERT INTO t VALUES (3)"}
        ]})),
        None,
    )
    .await;

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "UPDATE t SET v = v + 10"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["rows_affected"], 3,
        "UPDATE should report all affected rows"
    );
}

/// 目的:验证事务内 SELECT 的列/行结果正确返回。
#[tokio::test]
async fn sql_transaction_with_select() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": [
            {"sql": "CREATE TABLE t (v INTEGER)"},
            {"sql": "INSERT INTO t VALUES (42)"},
            {"sql": "SELECT v FROM t"}
        ]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tx with select: {body}");
    let results = body.as_array().unwrap();
    // DDL 的 rows_affected 由 SQLite 决定(此处为 1),不固定断言
    assert!(results[0]["rows_affected"].is_number());
    assert_eq!(results[1]["rows_affected"], 1);
    assert_eq!(results[2]["columns"], json!(["v"]));
    assert_eq!(results[2]["rows"], json!([[42]]));
}

/// 目的:验证事务原子性 —— 全部成功提交;任一失败整体回滚。
#[tokio::test]
async fn sql_transaction_atomicity() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE t (v TEXT)"})),
        None,
    )
    .await;

    // 全部成功
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": [
            {"sql": "INSERT INTO t VALUES ('a')"},
            {"sql": "INSERT INTO t VALUES ('b')"}
        ]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tx: {body}");
    assert_eq!(body.as_array().unwrap().len(), 2);

    // 第二条失败 → 整体回滚,第一条也不应生效
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": [
            {"sql": "INSERT INTO t VALUES ('c')"},
            {"sql": "INSERT INTO nosuch_table VALUES (1)"}
        ]})),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "failed tx should error: {body}"
    );

    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT COUNT(*) AS n FROM t"})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([[2]]), "rollback should keep 2 rows");
}

/// 目的:验证空 statements 的事务请求被拒绝(400)。
#[tokio::test]
async fn sql_transaction_requires_statements() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": []})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---- KV ----

/// 目的:验证 SET/GET/DEL/EXISTS/MGET/MSET 的基本读写语义。
#[tokio::test]
async fn kv_basic() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // SET → GET
    let (status, body) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/foo"),
        Some(json!({"value": "bar"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["written"], true);

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/foo"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["exists"], true);
    assert_eq!(body["value"], "bar");
    assert!(body.get("ttl_seconds").is_none());

    // 不存在的 key
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/nope"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["exists"], false);

    // EXISTS(批量)
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/exists"),
        Some(json!({"keys": ["foo", "nope"]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([true, false]));

    // MGET / MSET
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/mset"),
        Some(json!({"items": [{"key": "a", "value": "1"}, {"key": "b", "value": "2"}]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/mget"),
        Some(json!({"keys": ["a", "b", "foo"]})),
        None,
    )
    .await;
    assert_eq!(body["values"], json!(["1", "2", "bar"]));

    // DEL:第一次删除成功,第二次返回 deleted=false
    let (status, body) = send(
        &app,
        Method::DELETE,
        &format!("/v1/databases/{id}/kv/foo"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"], true);
    let (_, body) = send(
        &app,
        Method::DELETE,
        &format!("/v1/databases/{id}/kv/foo"),
        None,
        None,
    )
    .await;
    assert_eq!(body["deleted"], false);
}

/// 目的:验证 TTL 全链路 —— 写入带 TTL、GET 返回剩余秒数、
/// PERSIST(expire 不带 ttl)、重新 EXPIRE、TTL=0 立即过期(lazy expiration)、
/// 对不存在 key 执行 EXPIRE 返回 updated=false。
#[tokio::test]
async fn kv_ttl_and_expire() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // 带 TTL 写入
    let (_, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/tmp"),
        Some(json!({"value": "x", "ttl_seconds": 100})),
        None,
    )
    .await;

    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/tmp"),
        None,
        None,
    )
    .await;
    assert_eq!(body["exists"], true);
    let ttl = body["ttl_seconds"].as_i64().unwrap();
    assert!(
        (1..=100).contains(&ttl),
        "ttl should be within (0,100], got {ttl}"
    );

    // TTL 查询
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/ttl"),
        Some(json!({"keys": ["tmp"]})),
        None,
    )
    .await;
    assert!(body.as_array().unwrap()[0].as_i64().unwrap() > 0);

    // PERSIST(expire 不带 ttl)→ TTL 变 -1
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/expire"),
        Some(json!({"key": "tmp"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["updated"], true);
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/ttl"),
        Some(json!({"keys": ["tmp"]})),
        None,
    )
    .await;
    assert_eq!(
        body.as_array().unwrap()[0],
        -1,
        "persisted key should report -1"
    );

    // 重新设置 TTL
    let (_, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/expire"),
        Some(json!({"key": "tmp", "ttl_seconds": 50})),
        None,
    )
    .await;

    // lazy expiration:TTL 0 秒 → 立即过期,GET 视为不存在
    let (_, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/expired"),
        Some(json!({"value": "y", "ttl_seconds": 0})),
        None,
    )
    .await;
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/expired"),
        None,
        None,
    )
    .await;
    assert_eq!(body["exists"], false, "expired key should be invisible");

    // 对不存在的 key expire → updated=false
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/expire"),
        Some(json!({"key": "ghost", "ttl_seconds": 10})),
        None,
    )
    .await;
    assert_eq!(body["updated"], false);
}

/// 目的:验证 INCR/DECR 语义(从零开始、增量、负数即 DECR)、
/// 非整数值报 400,以及 SET NX / SET XX 条件写入。
#[tokio::test]
async fn kv_incr_and_nx_xx() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // INCR 从零开始
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "counter", "delta": 1})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], 1);

    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "counter", "delta": 10})),
        None,
    )
    .await;
    assert_eq!(body["value"], 11);

    // DECR
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "counter", "delta": -3})),
        None,
    )
    .await;
    assert_eq!(body["value"], 8);

    // 非整数 → 400
    send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/str"),
        Some(json!({"value": "hello"})),
        None,
    )
    .await;
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "str", "delta": 1})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // SET NX:key 已存在 → 不写入
    let (_, body) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/nxkey"),
        Some(json!({"value": "first", "nx": true})),
        None,
    )
    .await;
    assert_eq!(body["written"], true);
    let (_, body) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/nxkey"),
        Some(json!({"value": "second", "nx": true})),
        None,
    )
    .await;
    assert_eq!(body["written"], false, "NX should not overwrite");
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/nxkey"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"], "first");

    // SET XX:key 不存在 → 不写入
    let (_, body) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/xxkey"),
        Some(json!({"value": "x", "xx": true})),
        None,
    )
    .await;
    assert_eq!(body["written"], false, "XX should require existing key");
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/xxkey"),
        None,
        None,
    )
    .await;
    assert_eq!(body["exists"], false);
}

// ---- Lazy create 与 Active DB 上限 ----

/// 目的:验证 lazy create —— CREATE 不落盘,首次数据访问才创建 SQLite 文件。
#[tokio::test]
async fn lazy_create_creates_file_on_first_access() {
    let (app, _, dir) = test_app(16);
    let id = create_db(&app).await;

    // 创建后不应落盘
    let before: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flat_map(|e| e.ok())
        .collect();
    assert!(
        before.is_empty(),
        "lazy create must not touch disk: {before:?}"
    );

    // 首次 KV 写入触发文件创建
    send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/k"),
        Some(json!({"value": "v"})),
        None,
    )
    .await;
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flat_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1, "exactly one bucket dir expected: {files:?}");

    let sqlite_files: Vec<_> = files
        .iter()
        .flat_map(|d| std::fs::read_dir(d).unwrap().flat_map(|e| e.ok()))
        .filter(|e| e.path().extension().is_some_and(|x| x == "sqlite"))
        .collect();
    assert_eq!(
        sqlite_files.len(),
        1,
        "sqlite file should exist after first access"
    );
}

/// 目的:验证 Active DB Manager 的并发连接上限 —— 超过上限时按 LRU 逐出,
/// 被逐出的 db 再次访问自动重新打开。
#[tokio::test]
async fn active_connection_limit_with_lru_eviction() {
    let (app, client, _dir) = test_app(2);
    let db1 = create_db(&app).await;
    let db2 = create_db(&app).await;
    let db3 = create_db(&app).await;

    let touch = |id: &str| {
        let app = app.clone();
        let id = id.to_string();
        async move {
            send(
                &app,
                Method::POST,
                &format!("/v1/databases/{id}/sql"),
                Some(json!({"sql": "SELECT 1"})),
                None,
            )
            .await;
        }
    };

    touch(&db1).await;
    touch(&db2).await;
    assert_eq!(client.active_count(), 2);

    // 第三个 db 触发 LRU 逐出,连接数保持上限
    touch(&db3).await;
    assert_eq!(
        client.active_count(),
        2,
        "active connections must stay at the cap"
    );

    // 被逐出的 db 再次访问仍正常(重新打开)
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{db1}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ---- Auth 与访问控制 ----

/// 目的:验证 API key 认证 —— 未配置/错误 key 返回 401,正确 key 放行。
#[tokio::test]
async fn auth_requires_api_key() {
    let (app, _dir) = test_app_with_keys(&["test-key-1", "test-key-2"]).await;

    // 无 key → 401
    let (status, _) = send(&app, Method::GET, "/v1/databases", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 错误 key → 401
    let (status, _) = send(&app, Method::GET, "/v1/databases", None, Some("wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 正确 key → 200
    let (status, _) = send(&app, Method::GET, "/v1/databases", None, Some("test-key-2")).await;
    assert_eq!(status, StatusCode::OK);
}

/// 目的:验证访问控制 —— 访问 `__sys_*` 内部表被拒(403),
/// 事务控制语句被拒(400),非法 UUID 路径被拒(400)。
#[tokio::test]
async fn forbidden_statements_are_rejected() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // 访问内部表 → 403
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT * FROM __sys_kv"})),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "internal table access: {body}"
    );

    // 事务控制语句 → 400
    for bad in ["BEGIN", "COMMIT", "ROLLBACK", "BEGIN IMMEDIATE"] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": bad})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "should reject {bad}");
    }

    // 非法 UUID → 400
    let (status, _) = send(
        &app,
        Method::POST,
        "/v1/databases/not-a-uuid/sql",
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
