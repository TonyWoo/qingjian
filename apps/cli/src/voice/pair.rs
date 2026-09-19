//! 评测集里的一条：一个 WAV 与它的参考答案（同名 `.txt`）。

use std::path::{Path, PathBuf};

use crate::error::CliError;

/// 一条语音评测样本。
pub struct Pair {
    /// WAV 文件路径。
    pub wav: PathBuf,

    /// 参考答案（`.txt` 的内容，去掉首尾空白）。
    pub reference: String,
}

/// 扫一个目录：`<名字>.wav` 配 `<名字>.txt`。
/// 返回样本，以及「有 WAV 但没参考答案」的条数（后者只计数，不进 CER）。
pub fn scan(dir: &Path) -> Result<(Vec<Pair>, usize), CliError> {
    let mut pairs = Vec::new();
    let mut missing = 0;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
            continue;
        }
        let reference_path = path.with_extension("txt");
        if !reference_path.is_file() {
            missing += 1;
            continue;
        }
        pairs.push(Pair {
            reference: std::fs::read_to_string(&reference_path)?.trim().to_owned(),
            wav: path,
        });
    }
    pairs.sort_by(|a, b| a.wav.cmp(&b.wav));
    Ok((pairs, missing))
}
