//! 语音输入：找模型、接识别器、开麦克风、把识别结果攒成待上屏的文本。
//!
//! **上屏方式走「挂在下一个按键上」**：DLL 的按键路径本来就处理 `KeyResult.commit`
//! （`key_sink.rs` 收到就插进文档），所以 Server 只要把识别结果攒下来、等下一次被吃掉的
//! 按键捎过去就行 —— 不用改 IPC 协议、DLL 一行都不用动。
//!
//! 代价是**松手后要再按一下键文字才落**。之所以不接受「立刻上屏」：那要给 poll 加一条上屏
//! 通道（改协议 + 升版本），而且 DLL 录音期间根本不在轮询（`poll_once` 的守卫是「在组句或
//! 翻译评审中」），插字还得在非按键时机申请编辑会话 —— 三处联动，风险全压在真机上。
//! 短句输入这个场景下，说完顺手按一下空格本来就常见，先要正确再要快。
//!
//! 还有一条硬约束：**放行的功能键会把 commit 丢掉**（`key_sink.rs` 里 `consumed: false`
//! 且没有可打印字符的分支直接 `false`）。所以待上屏的文本只能交给「被吃掉」的按键。

mod router;

use std::path::{Path, PathBuf};

use qingjian_core::Engine;
use qingjian_voice::BackendConfig;

/// 语音输入的运行时。
#[derive(Default)]
pub struct Voice {
    /// 麦克风。没开成（没设备 / 没权限 / 没编进来）为 `None`，此时语音用不了。
    #[cfg(feature = "voice")]
    recorder: Option<qingjian_voice::audio::Recorder>,

    /// 试过开麦克风但失败了：只记一次日志，之后不再重试（免得每次按键都报一遍）。
    failed: bool,

    /// 识别好、等着搭下一次按键上屏的文本。
    pending: Option<String>,
}

impl Voice {
    /// 识别结果挂起中，还没有按键把它捎走。
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// 取走待上屏的文本。
    pub fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// 麦克风就绪了没有（第一次用到时才开）。
    pub fn ready(&self) -> bool {
        #[cfg(feature = "voice")]
        {
            self.recorder.is_some()
        }
        #[cfg(not(feature = "voice"))]
        {
            false
        }
    }

    /// 开麦克风。失败只记一次日志。
    pub fn ensure_recorder(&mut self) -> bool {
        if self.failed {
            return self.ready();
        }
        #[cfg(feature = "voice")]
        {
            if self.recorder.is_none() {
                match qingjian_voice::audio::Recorder::spawn() {
                    Ok(recorder) => {
                        tracing::info!("麦克风已就绪");
                        self.recorder = Some(recorder);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "开不了麦克风，语音输入用不了");
                        self.failed = true;
                    }
                }
            }
            self.recorder.is_some()
        }
        #[cfg(not(feature = "voice"))]
        {
            tracing::warn!("这个构建没编语音（缺 voice feature），语音输入用不了");
            self.failed = true;
            false
        }
    }

    /// 开始录音。
    pub fn start(&mut self) {
        #[cfg(feature = "voice")]
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.start();
        }
    }

    /// 停止录音（麦克风不关，下次接着用）。
    pub fn stop(&mut self) {
        #[cfg(feature = "voice")]
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.stop();
        }
    }

    /// 把采到的样本交给引擎，返回交出去多少秒（给日志用）。
    pub fn drain_into(&self, engine: &mut Engine) -> f32 {
        #[cfg(feature = "voice")]
        if let Some(recorder) = self.recorder.as_ref() {
            let mut seconds = 0.0;
            while let Some(samples) = recorder.poll() {
                seconds += samples.len() as f32 / qingjian_core::VOICE_SAMPLE_RATE as f32;
                engine.push_voice_samples(&samples);
            }
            return seconds;
        }
        // 没编语音时上面整段被 cfg 掉，engine 就成了未使用
        #[cfg(not(feature = "voice"))]
        let _ = engine;
        0.0
    }

    /// 取识别结果。到了就攒着等下一次按键，返回是否刚拿到。
    pub fn poll(&mut self, engine: &mut Engine) -> bool {
        let Some(text) = engine.poll_voice() else {
            return false;
        };
        self.pending = Some(text);
        true
    }

    /// 丢掉正在录的、在认的与待上屏的（进私密输入、切会话、关掉开关时调）。
    pub fn cancel(&mut self, engine: &mut Engine) {
        self.stop();
        engine.cancel_voice();
        if self.pending.take().is_some() {
            tracing::debug!("丢了还没上屏的语音文本");
        }
    }
}

/// 模型目录：先看用户数据目录 `voice/<档位>`，再退到随包（或开发布局）的 `data/voice/<档位>`。
///
/// 档位名留空时：先用 `voice.lock` 里的第一档，再退到「`voice/` 下第一个装了模型的子目录」——
/// 后者是给开发期用的（手动解压一个模型进去就能跑，不必先发布资产）。
pub fn find_model(user_dir: Option<&Path>, bundled_root: &Path, tier: &str) -> Option<PathBuf> {
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
        if let Some(name) = qingjian_voice::fetch::tiers().map(|(name, _)| name).next() {
            let dir = root.join(name);
            if looks_like_model(&dir) {
                return Some(dir);
            }
        }
        // 清单还是空的（资产没发布）时，认目录里第一个像模型的
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

/// 按模型目录造识别器并接到引擎上。加载失败只记日志、不接。
pub fn attach(engine: &mut Engine, model_dir: &Path, threads: i32) -> bool {
    let config = BackendConfig {
        model_dir: model_dir.to_path_buf(),
        threads,
    };
    match qingjian_voice::load_recognizer(&config) {
        Ok(recognizer) => {
            engine.set_speech_recognizer(Some(recognizer));
            tracing::info!(model = %model_dir.display(), threads, "语音识别已接上");
            true
        }
        Err(error) => {
            tracing::warn!(%error, model = %model_dir.display(), "语音模型加载失败，语音输入用不了");
            false
        }
    }
}

/// 卸掉识别器（开关关掉时调）。
pub fn detach(engine: &mut Engine) {
    engine.set_speech_recognizer(None);
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
    fn finds_the_model_by_tier_and_falls_back_to_scanning() {
        let root = std::env::temp_dir().join(format!("qingjian-voice-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tier = root.join("voice/base");
        std::fs::create_dir_all(&tier).unwrap();
        std::fs::write(tier.join("base-encoder.int8.onnx"), b"x").unwrap();
        // 随包目录指到同一个根：那里的 `data/voice` 不存在，不会替我们把结果兜住
        let bundled = root.clone();

        assert_eq!(
            find_model(Some(&root), &bundled, "base"),
            Some(tier.clone()),
            "给了档位就按档位找"
        );
        assert_eq!(
            find_model(Some(&root), &bundled, ""),
            Some(tier.clone()),
            "档位留空、清单也还是空的时候，认目录里第一个像模型的"
        );
        assert_eq!(
            find_model(Some(&root), &bundled, "small"),
            None,
            "档位对不上就是没找到"
        );
        // 空目录不算模型
        std::fs::remove_file(tier.join("base-encoder.int8.onnx")).unwrap();
        assert_eq!(find_model(Some(&root), &bundled, "base"), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
