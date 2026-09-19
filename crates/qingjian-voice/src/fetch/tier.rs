//! 一个档位：给用户看的名字与说明，加它要下载的文件列表。

use serde::Deserialize;

use super::Asset;

/// 一档模型（「标准」「精简」这类）。设置界面按档位列给用户选。
#[derive(Debug, Clone, Deserialize)]
pub struct Tier {
    /// 界面上显示的名字。
    pub label: String,

    /// 界面上的一行说明：体积、适用场景，让用户不用懂技术也能选。
    pub note: String,

    /// 这一档要下载的文件。
    pub files: Vec<Asset>,
}

impl Tier {
    /// 这一档下下来总共多少字节。
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    /// 界面用的体积文字，如「约 210 MB」。
    pub fn size_text(&self) -> String {
        format!("约 {:.0} MB", self.total_bytes() as f64 / (1024.0 * 1024.0))
    }
}
