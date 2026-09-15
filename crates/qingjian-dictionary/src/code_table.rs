//! 形码码表：按编码查字词（五笔）。
//!
//! 与 [`crate::Dictionary`] 的区别在键：词库按拼音音节序列查，码表按编码本身查，
//! 敲的编码是词的编码的前缀即可命中（`gan` 命中编码 `gant` 的 开发），没有音节也没有切分。
//!
//! 文件格式 TSV：
//!
//! ```text
//! 词\t编码\t词频
//! 开发\tgant\t9000
//! ```
//!
//! `#` 开头为注释行，空行忽略。编码统一按小写存，查询输入也须已小写。

use std::path::Path;

use crate::error::DictionaryError;
use crate::matching::Match;

/// 一条码表词目。
#[derive(Debug, Clone)]
struct Entry {
    /// 全码，小写。
    code: String,

    /// 词。
    text: String,

    /// 静态词频。
    frequency: u32,
}

/// 形码码表。词目按 `(编码, 词频降序)` 排好，同前缀的是一段连续区间。
#[derive(Debug, Default)]
pub struct CodeTable {
    /// 按编码字节序升序，同一个编码下按词频降序。
    entries: Vec<Entry>,

    /// 全部词频之和，上下文得分的兜底用。
    total_frequency: u64,
}

impl CodeTable {
    pub fn parse(source: &str) -> Result<Self, DictionaryError> {
        let mut entries: Vec<Entry> = Vec::new();
        for (index, raw) in source.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let mut fields = line.split('\t');
            let text = fields
                .next()
                .filter(|s| !s.is_empty())
                .ok_or(DictionaryError::Line {
                    line: number,
                    reason: "missing word",
                })?;
            let code = fields
                .next()
                .filter(|s| !s.is_empty())
                .ok_or(DictionaryError::Line {
                    line: number,
                    reason: "missing code",
                })?;
            let frequency = fields
                .next()
                .map(|s| s.trim().parse::<u32>())
                .transpose()
                .map_err(|_| DictionaryError::Line {
                    line: number,
                    reason: "frequency is not a non-negative integer",
                })?
                .unwrap_or(1);
            entries.push(Entry {
                code: code.to_ascii_lowercase(),
                text: text.to_owned(),
                frequency,
            });
        }
        entries.sort_by(|a, b| {
            a.code
                .cmp(&b.code)
                .then_with(|| b.frequency.cmp(&a.frequency))
                .then_with(|| a.text.cmp(&b.text))
        });
        // 同一个词记了两遍（不同码表版本合并）留词频高的那条，排完序就是靠前的那条
        entries.dedup_by(|a, b| a.code == b.code && a.text == b.text);
        let total_frequency = entries.iter().map(|e| u64::from(e.frequency)).sum();
        tracing::debug!(entries = entries.len(), "码表加载完成");
        Ok(Self {
            entries,
            total_frequency,
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DictionaryError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// 编码正好等于输入，或输入是它的前缀的词，最多 `limit` 条。
    ///
    /// 编码与输入相等（这个词打全了）的排在最前，其余按词频降序；`limit` 之后的不返回，
    /// 单字母输入（`g`）能命中几千条，排完序没人翻到后面。
    pub fn lookup(&self, code: &str, limit: usize) -> Vec<Match<'_>> {
        if code.is_empty() || limit == 0 {
            return Vec::new();
        }
        let start = self.entries.partition_point(|e| e.code.as_str() < code);
        let mut hits: Vec<&Entry> = self.entries[start..]
            .iter()
            .take_while(|e| e.code.starts_with(code))
            .collect();
        hits.sort_by(|a, b| {
            (b.code == code)
                .cmp(&(a.code == code))
                .then_with(|| b.frequency.cmp(&a.frequency))
                .then_with(|| a.code.cmp(&b.code))
                .then_with(|| a.text.cmp(&b.text))
        });
        hits.truncate(limit);
        hits.into_iter()
            .map(|entry| Match {
                text: entry.text.as_str(),
                pinyin: entry.code.as_str(),
                frequency: entry.frequency,
                exact: entry.code == code,
            })
            .collect()
    }

    /// 全部词频之和。
    pub fn total_frequency(&self) -> u64 {
        self.total_frequency
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_by_prefix_and_prefers_the_exact_code() {
        let table =
            CodeTable::parse("开发\tgant\t900\n开\tga\t5000\n一\tggll\t100000\n个\twhj\t8000\n")
                .unwrap();
        // 打全了的编码排在前缀命中的词前面，哪怕前缀词词频更高
        let hits = table.lookup("ga", 10);
        assert_eq!(
            hits.iter().map(|h| (h.text, h.exact)).collect::<Vec<_>>(),
            [("开", true), ("开发", false)]
        );
        // 没打全的编码只按词频
        let hits = table.lookup("g", 10);
        assert_eq!(
            hits.iter().map(|h| h.text).collect::<Vec<_>>(),
            ["一", "开", "开发"]
        );
        assert!(hits.iter().all(|h| !h.exact));
        assert!(table.lookup("zzz", 10).is_empty());
        assert!(table.lookup("", 10).is_empty());
        assert!(table.lookup("ga", 0).is_empty());
    }

    #[test]
    fn keeps_the_highest_frequency_of_a_duplicated_entry() {
        let table = CodeTable::parse("开发\tgant\t900\n开发\tgant\t3000\n").unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table.lookup("gant", 10)[0].frequency, 3000);
    }

    #[test]
    fn rejects_lines_without_a_code() {
        assert!(CodeTable::parse("开发\n").is_err());
        assert!(CodeTable::parse("开发\tgant\t不是数字\n").is_err());
    }

    #[test]
    fn normalizes_codes_to_lowercase() {
        let table = CodeTable::parse("开\tGA\t5000\n").unwrap();
        assert_eq!(table.lookup("ga", 10)[0].text, "开");
    }
}
