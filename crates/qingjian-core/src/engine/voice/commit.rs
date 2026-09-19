//! 语音文本上屏：繁体转换、个人 n-gram、输入日志与最近上屏。

use crate::engine::{Engine, InputSource, LastCommit};
use crate::sentence;

impl Engine {
    /// 语音识别出来的整段文本上屏。形状与 [`Self::accept_prediction`] 一样——都是「一段没有拼音的
    /// 整句文本」：记不了词频与用户词（没有音节可记），只按语言模型切成词逐条记进个人 n-gram。
    ///
    /// 识别器给的是简体。繁体模式下在这里做正向转换，并把「繁 → 简」记进 `traditional_map` ——
    /// 语音不经过 `query()`，没有候选那一步，所以正向转换与反向登记必须在这一处一次做完。
    /// 学习、输入日志、最近上屏都按简体原文走，返回给壳的是繁体。
    pub(super) fn commit_voice(&mut self, text: &str) -> String {
        let output = if self.traditional
            && let Some(opencc) = &self.opencc
        {
            let traditional = opencc.convert(text);
            self.traditional_map
                .borrow_mut()
                .insert(traditional.clone(), text.to_owned());
            traditional
        } else {
            text.to_owned()
        };

        // 语音没有组句，但 `log_commit` 会读上一次查询的摘要与重打快照：不清的话这条日志会带上
        // 上一段拼音的 scope / pinyin / top，还会凭 `retype_snapshot` 记一条没发生过的 retype。
        // `composition_started` 置空让日志里的 `ms` 定为 0（语音没有组句时长），而不是留着上一次的。
        *self.last_query.borrow_mut() = None;
        self.retype_snapshot = None;
        self.composition_started = None;
        self.apply_retraction("", text);
        self.recording.clear();

        let log_id = self.log_commit("", text, InputSource::Voice);
        self.meter_commit(text, InputSource::Voice, false);
        self.punctuation.note_committed(text);
        self.history.record(text);
        match sentence::segment_text(text, &*self.language_model) {
            Some(clauses) => {
                let count = clauses.len();
                for (index, words) in clauses.iter().enumerate() {
                    // 第一句接着前面上屏的词；标点之后的各句从句首起
                    if index > 0 {
                        self.chain.reset();
                    }
                    for word in words {
                        self.record_word(word, &[], 1, false, index + 1 < count);
                    }
                }
                if text.chars().last().is_some_and(|c| !c.is_alphanumeric()) {
                    self.chain.reset();
                }
                tracing::debug!(clauses = count, "语音文本已记入个人 n-gram");
            }
            None => self.chain.reset(),
        }

        let commit = LastCommit {
            text: text.to_owned(),
            chars: output.chars().count(),
            input: String::new(),
            chosen: None,
            transitions: std::mem::take(&mut self.recording),
            typos: Vec::new(),
            erased: 0,
            log_id,
            phrase: None,
        };
        self.remember_commit(commit);

        // 这个映射只在 `clear()` 与 `take_raw()` 里清，语音两条路都不走，不清会一直涨。
        self.traditional_map.borrow_mut().clear();
        output
    }
}
