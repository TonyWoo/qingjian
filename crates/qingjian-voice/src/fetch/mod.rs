//! 语音模型的下载：按内置清单逐文件下载、校验 SHA256、原子落盘。
//!
//! 模型不进安装包（Whisper 这类几百 MB，而多数用户不用语音），改成在设置里按需下载。
//! 清单是 crate 根下的 `voice.lock`，用 `include_str!` 编进二进制 —— 设置界面因此不需要
//! 去磁盘上找它，也就不会出现「清单与二进制对不上」这种事。

mod asset;
mod download;
mod tier;

pub use asset::Asset;
pub use download::{Download, FetchEvent};
pub use tier::Tier;

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// 编进二进制的下载清单，由 `tools/release/pack-voice.sh` 生成。
const LOCK: &str = include_str!("../../voice.lock");

/// 清单：档位名 → 档位。解析一次就缓存住。
fn catalog() -> &'static BTreeMap<String, Tier> {
    static CATALOG: OnceLock<BTreeMap<String, Tier>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(LOCK).expect("voice.lock 解析不了：它是编进二进制的，坏了就是构建错了")
    })
}

/// 按名字取一档；没有这一档返回 `None`。
pub fn tier(name: &str) -> Option<&'static Tier> {
    catalog().get(name)
}

/// 所有档位，按名字排序（`BTreeMap` 保证）。设置界面按它列档位。
pub fn tiers() -> impl Iterator<Item = (&'static str, &'static Tier)> {
    catalog().iter().map(|(name, tier)| (name.as_str(), tier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_lock_parses() {
        // 解析不了会在 catalog() 里 panic
        let _ = catalog();
    }

    #[test]
    fn an_unknown_tier_is_none() {
        assert!(tier("no-such-tier").is_none());
    }
}
