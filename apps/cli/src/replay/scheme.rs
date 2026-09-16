//! 回放期间按日志里记的方案装配引擎。
//!
//! 每条上屏的 `scheme` 字段写着当时用的是哪套方案（双拼的键、`zhuyin`、形码的 `wubi`、全拼为空），
//! 回放要照着还原，否则拿拼音的读法去喂形码的键、或者反过来，算出来的命中率没有意义。
//! 整份日志通常只有一套方案，所以只在方案串变化时才动码表——`Engine::set_code_table` 收所有权，
//! 换一次要克隆一份 8.9 万条的码表。

use qingjian_core::Engine;
use qingjian_dictionary::CodeTable;
use qingjian_platform::Scheme;

/// 按日志里的方案串装配引擎，并记住当前装的是哪套。
pub struct SchemeSwitcher {
    /// 当前已装配的方案串；没装过是 `None`。
    current: Option<String>,

    /// 日志里出现形码时用的码表；没给 `--wubi` 就是 `None`，那部分日志回放不了。
    table: Option<CodeTable>,
}

impl SchemeSwitcher {
    pub fn new(table: Option<CodeTable>) -> Self {
        Self {
            current: None,
            table,
        }
    }

    /// 这条日志能不能回放：形码要有码表，别的方案都行。
    pub fn can_replay(&self, scheme: &str) -> bool {
        !parse(scheme).is_code() || self.table.is_some()
    }

    /// 按 `scheme` 装配引擎。双拼与注音的开关每次都设（便宜）；码表只在方案串变了时换。
    pub fn apply(&mut self, engine: &mut Engine, scheme: &str) {
        let parsed = parse(scheme);
        engine.set_shuangpin(parsed.shuangpin());
        engine.set_zhuyin_mode(parsed == Scheme::Zhuyin);
        if self.current.as_deref() == Some(scheme) {
            return;
        }
        self.current = Some(scheme.to_owned());
        engine.set_code_table(if parsed.is_code() {
            self.table.clone()
        } else {
            None
        });
    }
}

/// 日志里的方案串解成 [`Scheme`]；空串与认不出来的都按全拼（老日志里没有这个字段）。
fn parse(scheme: &str) -> Scheme {
    scheme.parse().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_which_schemes_need_a_table() {
        let without = SchemeSwitcher::new(None);
        assert!(without.can_replay(""));
        assert!(without.can_replay("xiaohe"));
        assert!(without.can_replay("zhuyin"));
        assert!(!without.can_replay("wubi"));

        let table = CodeTable::parse("一\tggll\t100\n").unwrap();
        assert!(SchemeSwitcher::new(Some(table)).can_replay("wubi"));
    }

    #[test]
    fn applying_a_code_scheme_hands_the_table_to_the_engine() {
        use qingjian_core::Engine;
        use qingjian_dictionary::Dictionary;

        let table = CodeTable::parse("一\tggll\t100\n").unwrap();
        let mut switcher = SchemeSwitcher::new(Some(table));
        let mut engine = Engine::new(Dictionary::parse("开\tkai\t100\n").unwrap());

        switcher.apply(&mut engine, "wubi");
        engine.set_input("ggll");
        assert_eq!(engine.query().unwrap().candidates.items[0].text, "一");

        // 切回全拼：码表卸掉，同一串编码不再出中文候选
        switcher.apply(&mut engine, "");
        engine.set_input("ggll");
        let texts: Vec<String> = engine
            .query()
            .map(|q| q.candidates.items.into_iter().map(|c| c.text).collect())
            .unwrap_or_default();
        assert!(!texts.contains(&"一".to_owned()));
    }

    #[test]
    fn empty_and_unknown_scheme_strings_fall_back_to_pinyin() {
        // 老日志没有 scheme 字段，解出来是全拼
        assert_eq!(parse(""), Scheme::Pinyin);
        assert_eq!(parse("wubi"), Scheme::Wubi86);
        assert_eq!(
            parse("xiaohe"),
            Scheme::Shuangpin(qingjian_core::ShuangpinScheme::Xiaohe)
        );
        assert_eq!(parse("没见过的写法"), Scheme::Pinyin);
    }
}
