//! SQL 沙箱:引擎层(authorizer)防线。
//!
//! 为什么不用字符串黑名单:黑名单(`starts_with`/`contains`)是在和 SQLite 的
//! 词法/语法解析打地鼠 —— 前导注释、Unicode 空白、嵌套注释、大小写、`/*!*/`
//! 变体等绕过方式源源不断。authorizer 是 SQLite 为"沙箱化"设计的正道:
//! 每条语句在 **prepare 阶段**由引擎回调逐动作授权,语法如何变都不影响判定。
//!
//! 与内部操作的区分:Data Node 里同一个 SQLite 连接既服务用户 SQL,也服务
//! 内部操作(内部 KV 的 `__sys_kv`、schema 初始化、备份/恢复的 VACUUM)。
//! authorizer 是连接级回调,无法知道"这条语句来自用户还是内部",因此用
//! 线程本地标志 [`UserSqlGuard`] 标记"当前线程正在执行用户 SQL":
//! 仅当标志置位时执行拦截,内部操作一律放行。
//!
//! authorizer 覆盖不到的残留(无对应 action code 的语句)由 `sql::check_statement`
//! 的字符串层补充:`VACUUM`(无 authorizer action)、多语句注入(`SELECT 1; DROP …`)。

use std::cell::Cell;

use rusqlite::Connection;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

thread_local! {
    /// 当前线程是否处于"用户 SQL 上下文"(用户 SQL 执行期间置 true;
    /// 内部 KV / schema 初始化 / 备份恢复不置位)。见模块文档。
    static USER_SQL_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// 用户 SQL 上下文守卫:进入时把线程标志置为给定值,drop 时恢复进入前的值。
/// 嵌套使用安全(保存/恢复的是"上一状态",不是常量)。
pub struct UserSqlGuard {
    prev: bool,
}

impl UserSqlGuard {
    fn set(active: bool) -> Self {
        let prev = USER_SQL_ACTIVE.with(|f| f.replace(active));
        Self { prev }
    }

    /// 进入用户 SQL 上下文(后续该线程上的语句会被沙箱授权)。
    pub fn enter() -> Self {
        Self::set(true)
    }

    /// 临时退出用户 SQL 上下文(如事务的 BEGIN/COMMIT 由引擎/rusqlite 内部
    /// 发出,必须放行);drop 时恢复。
    pub fn leave() -> Self {
        Self::set(false)
    }

    /// 当前线程是否处于用户 SQL 上下文(authorizer 回调读取)。
    pub fn is_active() -> bool {
        USER_SQL_ACTIVE.with(|f| f.get())
    }
}

impl Drop for UserSqlGuard {
    fn drop(&mut self) {
        USER_SQL_ACTIVE.with(|f| f.set(self.prev));
    }
}

/// 允许用户执行的 pragma(白名单;默认拒绝其余所有 pragma)。
///
/// 分三类:
/// - **函数式**(`PRAGMA table_info(t)` 等):括号参数只是查询参数,无副作用,放行;
/// - **只读查询式**(`PRAGMA journal_mode` 等):仅无值(查询)放行;
/// - **可安全赋值**:仅 `user_version` / `application_id`(存用户元数据)。
fn pragma_allowed(name: &str, has_value: bool) -> bool {
    // 函数式 pragma:参数是表/索引名等查询对象,不带副作用。
    let functional = matches!(
        name,
        "quick_check"
            | "integrity_check"
            | "table_info"
            | "table_xinfo"
            | "table_list"
            | "database_list"
            | "index_list"
            | "index_info"
            | "index_xinfo"
            | "collation_list"
            | "compile_options"
            | "function_list"
            | "module_list"
            | "pragma_list"
    );
    if functional {
        return true;
    }
    // 只读查询式:无值(查询)放行;带赋值仅 user_version/application_id 允许。
    let read_only = matches!(
        name,
        "user_version"
            | "application_id"
            | "page_count"
            | "freelist_count"
            | "schema_version"
            | "data_version"
            | "journal_mode" // 只允许查询当前模式,不允许修改(WAL 持久性)
            | "foreign_keys"
            | "wal_checkpoint"
    );
    if !read_only {
        return false;
    }
    if has_value {
        return matches!(name, "user_version" | "application_id");
    }
    true
}

/// 危险函数黑名单:任意文件读写 / 扩展加载(CLI-only 或可加载扩展时越权)。
/// 其余内置函数(aggregate/date/random 等)与用户注册函数一律放行。
fn function_allowed(name: &str) -> bool {
    !matches!(
        name,
        "load_extension" | "readfile" | "writefile" | "sqlite_compileoption_used"
    )
}

/// 安装 SQL 沙箱 authorizer 到给定连接。
///
/// 必须在连接初始化完成(schema / pragma / quick_check)之后调用;
/// 卸载传 `None`(rusqlite 用 `authorizer(None::<fn(_) -> _>)`)。
pub fn install(conn: &Connection) {
    conn.authorizer(Some(|ctx: AuthContext<'_>| {
        // 内部操作(非用户 SQL 上下文)一律放行 —— 见模块文档。
        if !UserSqlGuard::is_active() {
            return Authorization::Allow;
        }
        match ctx.action {
            // 附加/分离数据库:改变连接作用域,破坏连接复用模型且可逃逸文件系统。
            AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
            // 事务控制/SAVEPOINT:事务必须走 /transaction 端点(引擎内部发出
            // 的 BEGIN/COMMIT 处于非用户上下文,不受此限制)。
            AuthAction::Transaction { .. } | AuthAction::Savepoint { .. } => Authorization::Deny,
            AuthAction::Pragma {
                pragma_name,
                pragma_value,
            } => {
                let name = pragma_name.to_ascii_lowercase();
                if pragma_allowed(&name, pragma_value.is_some()) {
                    Authorization::Allow
                } else {
                    Authorization::Deny
                }
            }
            AuthAction::Function { function_name } => {
                if function_allowed(&function_name.to_ascii_lowercase()) {
                    Authorization::Allow
                } else {
                    Authorization::Deny
                }
            }
            // 内部表(`__sys_*`)对用户完全不可见:读/写一律拒绝。
            AuthAction::Read { table_name, .. }
            | AuthAction::Insert { table_name }
            | AuthAction::Update { table_name, .. }
            | AuthAction::Delete { table_name } => {
                if table_name.starts_with("__sys") {
                    Authorization::Deny
                } else {
                    Authorization::Allow
                }
            }
            // 其余(建表/索引/视图、SELECT、DROP 等)在用户自己的 Cell 内放行。
            _ => Authorization::Allow,
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pragma_whitelist_judgment() {
        // 只读查询放行
        for p in [
            "user_version",
            "quick_check",
            "integrity_check",
            "table_info",
            "journal_mode",
            "foreign_keys",
            "function_list",
            "pragma_list",
        ] {
            assert!(pragma_allowed(p, false), "query {p} should be allowed");
        }
        // 函数式 pragma 带括号参数(table_info(t))仍放行
        for p in ["table_info", "quick_check", "index_list", "table_list"] {
            assert!(
                pragma_allowed(p, true),
                "functional {p} with arg should be allowed"
            );
        }
        // 危险 pragma(带赋值或本身危险)拒绝;journal_mode 只允许无值查询
        for p in [
            "synchronous",
            "locking_mode",
            "temp_store",
            "page_size",
            "cache_size",
            "wal_autocheckpoint",
            "trusted_schema",
            "load_extension",
            "security_delete",
            "read_uncommitted",
        ] {
            assert!(!pragma_allowed(p, true), "set {p} should be denied");
            assert!(!pragma_allowed(p, false), "query {p} should also be denied");
        }
        // journal_mode:查询放行,赋值拒绝(WAL 持久性不可由用户改)
        assert!(pragma_allowed("journal_mode", false));
        assert!(!pragma_allowed("journal_mode", true));
        // user_version / application_id 允许赋值(存用户元数据)
        assert!(pragma_allowed("user_version", true));
        assert!(pragma_allowed("application_id", true));
    }

    #[test]
    fn function_blacklist_judgment() {
        for f in ["load_extension", "readfile", "writefile"] {
            assert!(!function_allowed(f), "{f} should be denied");
        }
        for f in [
            "count",
            "sum",
            "avg",
            "max",
            "min",
            "random",
            "date",
            "strftime",
            "json_extract",
            "lower",
            "upper",
            "length",
        ] {
            assert!(function_allowed(f), "{f} should be allowed");
        }
    }
}
