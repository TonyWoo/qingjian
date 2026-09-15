//! 形码码表：Rime `.dict.yaml`（极点 86 五笔等）→ `词\t编码\t词频`。
//!
//! 码表自带的第二列（Rime 里是权重）**不用**：那是码表顺序，不是语料词频，直接拿来排序会让同一个词
//! 在形码下和在拼音下排得不一样。词频从青简词库（`词\t拼音\t词频`）按**词面**交叉回填：
//! 词库里有的用语料词频（与拼音方案同一把尺子，释义兜底与生词识别也对得上），
//! 没有的给 [`UNKNOWN_FREQUENCY`]——语料里一次都没出现过，本来就该排在同编码的已知词后面。
//!
//! 词组编码（二字词 2+2、三字词 1+1+1）由上游码表给全，这里不做取码推导，见 `docs/plan/wubi.md`。

use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::Path;

use qingjian_dictionary::Dictionary;
use qingjian_dictionary::import::{looks_like_rime, to_tsv};

use crate::error::ConvertError;

/// 词库里查不到的词给的词频。
pub const UNKNOWN_FREQUENCY: u32 = 1;

/// 转换结果，打日志用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Converted {
    /// 写出的词条数。
    pub entries: usize,

    /// 其中词频来自语料的条数。
    pub with_frequency: usize,

    /// 词库里查不到、拿了 [`UNKNOWN_FREQUENCY`] 的条数。占比高说明码表与词库的词面写法对不上，要查。
    pub unknown: usize,
}

/// 读 Rime 码表写出青简形码 TSV，词频从 `frequency`（青简词库 TSV）交叉回填。
pub fn convert(input: &Path, frequency: &Path, output: &Path) -> Result<Converted, ConvertError> {
    let source = std::fs::read_to_string(input)?;
    if !looks_like_rime(&source) {
        return Err(ConvertError::Format {
            path: input.to_owned(),
            line: 0,
            reason: "expected a Rime .dict.yaml (a `---` header or a `name:` line)".to_owned(),
        });
    }
    let dictionary = Dictionary::from_path(frequency)?;
    let frequencies: HashMap<&str, u32> = dictionary
        .entries()
        .map(|hit| (hit.text, hit.frequency))
        .collect();

    let tsv = to_tsv(&source).tsv;
    let mut entries: Vec<(String, String, u32)> = Vec::new();
    for (index, raw) in tsv.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let (Some(word), Some(code)) = (fields.next(), fields.next()) else {
            return Err(ConvertError::Format {
                path: input.to_owned(),
                line: index + 1,
                reason: "expected `word<TAB>code`".to_owned(),
            });
        };
        let (word, code) = (word.trim(), code.trim().to_ascii_lowercase());
        if word.is_empty() || code.is_empty() {
            continue;
        }
        let weight = frequencies.get(word).copied().unwrap_or(UNKNOWN_FREQUENCY);
        entries.push((word.to_owned(), code, weight));
    }
    // 按 (编码, 词频降序, 词) 排：与 `CodeTable` 在内存里的顺序一致，文件本身也一眼看得出排序；
    // 同一个 (词, 编码) 出现两次（合并多份码表）留第一次
    entries.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    entries.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let unknown = entries
        .iter()
        .filter(|(_, _, frequency)| *frequency == UNKNOWN_FREQUENCY)
        .count();
    let mut writer = BufWriter::new(std::fs::File::create(output)?);
    writeln!(
        writer,
        "# 形码码表：词\t编码\t词频。词频由青简词库（assets/lexicon/dict.tsv）按词面回填，不是码表自带的权重"
    )?;
    for (word, code, frequency) in &entries {
        writeln!(writer, "{word}\t{code}\t{frequency}")?;
    }
    writer.flush()?;
    Ok(Converted {
        entries: entries.len(),
        with_frequency: entries.len() - unknown,
        unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"---
name: wubi86
...
的	r	100
一	ggll	90
开	ga	80
开发	gant	70
不给	gqt	60
"#;

    /// 词库里有 的 / 开 / 开发 / 一，码表里的 不给 不在其中，用来验证按词面回填与兜底。
    const DICT: &str = "的\tde\t5000\n开\tkai\t800\n开发\tkai fa\t900\n一\tyi\t4000\n";

    /// 每个用例一个自己的目录：`cargo test` 并行跑，共用目录会互相覆盖。
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qingjian-wubi-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn convert_to_strings(name: &str, table: &str, dict: &str) -> (Vec<String>, Converted) {
        let dir = scratch(name);
        std::fs::create_dir_all(&dir).unwrap();
        let table_path = dir.join("table.dict.yaml");
        let dict_path = dir.join("dict.tsv");
        let out_path = dir.join("wubi86.tsv");
        std::fs::write(&table_path, table).unwrap();
        std::fs::write(&dict_path, dict).unwrap();
        let converted = convert(&table_path, &dict_path, &out_path).unwrap();
        let lines: Vec<String> = std::fs::read_to_string(&out_path)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .map(str::to_owned)
            .collect();
        (lines, converted)
    }

    #[test]
    fn fills_frequency_by_word_and_sorts_by_code() {
        let (lines, converted) = convert_to_strings("fills", TABLE, DICT);
        assert_eq!(
            lines,
            [
                "开\tga\t800",
                "开发\tgant\t900",
                "一\tggll\t4000",
                "不给\tgqt\t1",
                "的\tr\t5000",
            ]
        );
        assert_eq!(
            converted,
            Converted {
                entries: 5,
                with_frequency: 4,
                unknown: 1,
            }
        );
    }

    #[test]
    fn words_missing_from_the_dictionary_get_the_floor_frequency() {
        let (lines, converted) =
            convert_to_strings("floor", TABLE, "差不多的词\tcha bu duo de ci\t9\n");
        assert!(lines.iter().all(|l| l.ends_with("\t1")));
        assert_eq!(converted.unknown, 5);
        assert_eq!(converted.with_frequency, 0);
    }

    #[test]
    fn keeps_the_highest_frequency_when_a_word_appears_twice() {
        let (lines, _) =
            convert_to_strings("dupes", "---\nname: t\n...\n开发\tgant\n开发\tgant\n", DICT);
        assert_eq!(lines, ["开发\tgant\t900"]);
    }

    #[test]
    fn rejects_files_that_are_not_rime_dictionaries() {
        let dir = scratch("not-rime");
        let table = dir.join("plain.tsv");
        let dict_path = dir.join("dict.tsv");
        std::fs::write(&table, "开\tga\t800\n").unwrap();
        std::fs::write(&dict_path, DICT).unwrap();
        assert!(convert(&table, &dict_path, &dir.join("out.tsv")).is_err());
    }
}
