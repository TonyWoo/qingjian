//! 语音评测报告：总 CER、RTF、空结果与最差的几条。

use std::fmt::Write as _;

/// 一条评测结果。
pub struct Row {
    pub name: String,

    pub reference: String,

    /// `None` 表示识别器没给出结果（静音 / 识别不出）。
    pub hypothesis: Option<String>,

    pub distance: usize,

    pub reference_chars: usize,

    /// 音频时长（秒）。
    pub seconds: f32,

    /// 从「松手」到拿到结果花了多久。
    pub millis: u128,
}

/// 一份评测报告。
#[derive(Default)]
pub struct Report {
    pub rows: Vec<Row>,

    /// 有 WAV 但没有参考答案的条数。
    pub missing_reference: usize,

    /// 音频读不了、跳过没算的条数（采样率不对、文件坏了）。一条坏文件不该中断整场评测。
    pub skipped: usize,
}

impl Report {
    /// 渲染成给人看的文本，列出最差的 `misses` 条。
    pub fn render(&self, misses: usize) -> String {
        let mut out = String::new();
        let distance: usize = self.rows.iter().map(|row| row.distance).sum();
        let chars: usize = self.rows.iter().map(|row| row.reference_chars).sum();
        let seconds: f32 = self.rows.iter().map(|row| row.seconds).sum();
        let millis: u128 = self.rows.iter().map(|row| row.millis).sum();
        let empty = self
            .rows
            .iter()
            .filter(|row| row.hypothesis.is_none())
            .count();

        writeln!(out, "语音评测：{} 条", self.rows.len()).ok();
        writeln!(
            out,
            "总 CER {:.2}%（编辑距离 {distance} / 参考 {chars} 字）",
            percent(distance, chars)
        )
        .ok();
        writeln!(
            out,
            "RTF 合计 {:.3}（音频 {seconds:.1} s，识别 {millis} ms）",
            rtf(millis, seconds)
        )
        .ok();
        if let Some(slowest) = self
            .rows
            .iter()
            .max_by(|a, b| rtf(a.millis, a.seconds).total_cmp(&rtf(b.millis, b.seconds)))
        {
            writeln!(
                out,
                "最慢一条 {}：RTF {:.3}（{} ms / {:.1} s）",
                slowest.name,
                rtf(slowest.millis, slowest.seconds),
                slowest.millis,
                slowest.seconds
            )
            .ok();
        }
        if empty > 0 {
            writeln!(out, "空结果 {empty} 条").ok();
        }
        if self.missing_reference > 0 {
            writeln!(
                out,
                "有 WAV 但没有参考答案 {} 条，未计入",
                self.missing_reference
            )
            .ok();
        }
        if self.skipped > 0 {
            writeln!(out, "音频读不了跳过 {} 条，未计入", self.skipped).ok();
        }

        if misses > 0 && !self.rows.is_empty() {
            let mut ranked: Vec<&Row> = self.rows.iter().collect();
            ranked.sort_by(|a, b| cer_of(b).total_cmp(&cer_of(a)));
            let listed = misses.min(ranked.len());
            writeln!(out, "\n最差的 {listed} 条：").ok();
            for row in ranked.iter().take(listed) {
                writeln!(out, "  {}  CER {:.1}%", row.name, cer_of(row)).ok();
                writeln!(out, "    参考 {}", row.reference).ok();
                writeln!(
                    out,
                    "    识别 {}",
                    row.hypothesis.as_deref().unwrap_or("（空）")
                )
                .ok();
            }
        }
        out
    }
}

/// 实时率：识别耗时 / 音频时长。大于 1 就是说完了还要等。
pub fn rtf(millis: u128, seconds: f32) -> f64 {
    if seconds <= 0.0 {
        0.0
    } else {
        millis as f64 / 1000.0 / seconds as f64
    }
}

fn cer_of(row: &Row) -> f64 {
    percent(row.distance, row.reference_chars)
}

fn percent(distance: usize, chars: usize) -> f64 {
    if chars == 0 {
        0.0
    } else {
        distance as f64 * 100.0 / chars as f64
    }
}
