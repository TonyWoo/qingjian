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
use std::path::{Path, PathBuf};
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

/// 本机已经装好的模型目录：用户数据目录 `voice/<档位>` 优先，随包（或开发布局）的
/// `data/voice/<档位>` 兜底。
///
/// 档位留空时：先用清单里的第一档，清单还空就认目录里第一个装了模型的 —— 后者是给开发期用的
/// （手动解压一个模型进去就能跑，不必先发布资产）。
///
/// Server 与设置界面共用这一份，免得两边对「模型装在哪」的判断漂移。
pub fn installed(user_dir: Option<&Path>, bundled_root: &Path, tier: &str) -> Option<PathBuf> {
    let roots = [
        user_dir.map(|dir| dir.join("voice")),
        Some(bundled_root.join("data/voice")),
    ];
    for root in roots.into_iter().flatten() {
        if !tier.is_empty() {
            let dir = root.join(tier);
            if looks_like_model(&dir) {
                return Some(dir);
            }
            continue;
        }
        if let Some(name) = tiers().map(|(name, _)| name).next() {
            let dir = root.join(name);
            if looks_like_model(&dir) {
                return Some(dir);
            }
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| looks_like_model(path))
            .collect();
        entries.sort();
        if let Some(dir) = entries.into_iter().next() {
            return Some(dir);
        }
    }
    None
}

/// 一个目录像不像模型：里面至少有一个 `.onnx`。
fn looks_like_model(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".onnx"))
        })
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

    #[test]
    fn installed_finds_by_tier_and_falls_back_to_scanning() {
        let root = std::env::temp_dir().join(format!("qingjian-voice-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let model = root.join("voice/base");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("base-encoder.int8.onnx"), b"x").unwrap();
        // 随包目录指到同一个根：那里的 `data/voice` 不存在，不会替我们把结果兜住
        let bundled = root.clone();

        assert_eq!(
            installed(Some(&root), &bundled, "base"),
            Some(model.clone()),
            "给了档位就按档位找"
        );
        assert_eq!(
            installed(Some(&root), &bundled, ""),
            Some(model.clone()),
            "档位留空、清单也还是空的时候，认目录里第一个像模型的"
        );
        assert_eq!(
            installed(Some(&root), &bundled, "small"),
            None,
            "档位对不上就是没找到"
        );

        // 空目录不算模型
        std::fs::remove_file(model.join("base-encoder.int8.onnx")).unwrap();
        assert_eq!(installed(Some(&root), &bundled, "base"), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
