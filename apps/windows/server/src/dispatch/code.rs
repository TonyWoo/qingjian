//! 形码码表的装载：`[general] scheme` 选了形码时，把码表找出来挂到引擎上。
//!
//! 与本地整句模型同一套找法（见 [`super::find_model`]）：用户目录优先，随包数据兜底。
//! 选形码但码表不在时不静默按拼音跑——配置说五笔、引擎还在拼音是很糟的错位，宁可在日志里喊出来。

use std::path::{Path, PathBuf};

use qingjian_core::Engine;
use qingjian_dictionary::CodeTable;
use qingjian_platform::Scheme;

use super::Router;

/// 用户目录下的码表（用户自己换的那份）。
const USER_TABLE: &str = "wubi/wubi86.tsv";

/// 随包数据里的码表。
const BUNDLED_TABLE: &str = "data/wubi/wubi86.tsv";

/// 找码表：用户目录优先，否则随包数据；都没有为 `None`。
pub fn find_code_table(user_dir: Option<&Path>, bundled_root: &Path) -> Option<PathBuf> {
    user_dir
        .map(|dir| dir.join(USER_TABLE))
        .filter(|path| path.is_file())
        .or_else(|| {
            let bundled = bundled_root.join(BUNDLED_TABLE);
            bundled.is_file().then_some(bundled)
        })
}

/// 按 `scheme` 装配引擎的形码码表：不是形码就卸掉；是形码但码表不在就警告并保持拼音。
/// `table` 是启动时用 [`find_code_table`] 找好的路径（与本地模型一样，热加载时不重新找）。
fn apply_scheme(engine: &mut Engine, scheme: Scheme, table: Option<&Path>) {
    if !scheme.is_code() {
        engine.set_code_table(None);
        return;
    }
    let Some(path) = table else {
        tracing::warn!(
            table = BUNDLED_TABLE,
            "选了形码方案但找不到码表，仍按拼音输入；随包数据里应当带一份",
        );
        engine.set_code_table(None);
        return;
    };
    match CodeTable::from_path(path) {
        Ok(table) => {
            tracing::info!(table = %path.display(), entries = table.len(), "形码码表已载入");
            engine.set_code_table(Some(table));
        }
        Err(error) => {
            tracing::error!(%error, table = %path.display(), "形码码表读不了，仍按拼音输入");
            engine.set_code_table(None);
        }
    }
}

impl Router {
    /// 启动时把找好的码表路径交给路由器，并按当前方案装配一次。
    pub fn configure_code_table(&mut self, table: Option<PathBuf>) {
        self.code_table = table;
        apply_scheme(
            &mut self.engine,
            self.config.scheme,
            self.code_table.as_deref(),
        );
    }

    /// 热加载：方案变了就按同一个路径重新装配（不重新找文件，与本地模型一致）。
    pub(super) fn reload_code_table(&mut self, scheme: Scheme) {
        apply_scheme(&mut self.engine, scheme, self.code_table.as_deref());
    }
}
