//! 语音输入：找模型、接识别器、开麦克风、把识别结果攒成待选的候选。
//!
//! **上屏方式走「候选窗里选一条」**（2026-09-19 定，改掉了原先的「挂在下一个按键上」）：
//! 识别好了 Server 把文字画成候选窗里的第 1 条（帧上的 [`VoicePrompt`]，见 `dispatch/composed`），
//! 用户按空格 / `1` 接受、`Esc` 丢弃 —— 只有 DLL 能把键转进来，所以帧上要带语音状态，
//! DLL 才知道该拦空格（`Frame::is_empty` 把语音算作空，它不会误判成在组句）。
//!
//! 为什么不直接上屏：Server 没有通道能主动往文档里写字，只有 DLL 在按键路径上能写
//! （`key_sink.rs` 收到 `KeyResult.commit` 就插进文档）。要在识别一完成就落字，得给 poll 加一条
//! 上屏通道 + 让 DLL 在非按键时机申请编辑会话插字 —— 那两处联动风险全压在真机上，
//! 而且用户看不见识别成了什么、也没法反悔。多按一下空格换「看得见、能反悔」，值。

mod router;

use std::path::Path;

use qingjian_core::{Engine, VoiceState};
use qingjian_platform::protocol::VoicePrompt;
use qingjian_voice::BackendConfig;

/// 语音输入的运行时。
#[derive(Default)]
pub struct Voice {
    /// 麦克风。没开成（没设备 / 没权限 / 没编进来）为 `None`，此时语音用不了。
    #[cfg(feature = "voice")]
    recorder: Option<qingjian_voice::audio::Recorder>,

    /// 试过开麦克风但失败了：只记一次日志，之后不再重试（免得每次按键都报一遍）。
    failed: bool,

    /// 识别好、摆在候选窗里等用户选的文本。
    pending: Option<String>,
}

/// 用户在语音候选上按的那一下。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceChoice {
    /// 接受：这一段上屏。
    Accept,

    /// 丢弃（`Esc`）：不上屏，也不学习。
    Discard,
}

impl Voice {
    /// 识别结果还挂着，等用户选。
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// 取走待选的文本（接受那一下用；取走之前先 [`Engine::accept_voice`] 才走上屏）。
    pub fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// 丢掉待选的文本（`Esc`、敲了别的键、切走会话）。
    pub fn discard(&mut self) {
        if self.pending.take().is_some() {
            tracing::info!("语音结果被丢弃");
        }
    }

    /// 帧上要带的语音状态：在认 / 等选。两个都不是返回 `None`。
    pub fn prompt(&self, state: VoiceState) -> Option<VoicePrompt> {
        match &self.pending {
            Some(text) => Some(VoicePrompt::Ready { text: text.clone() }),
            None if state == VoiceState::Transcribing => Some(VoicePrompt::Transcribing),
            None => None,
        }
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

    /// 取识别结果。到了就摆着等用户选，返回是否刚拿到。
    pub fn poll(&mut self, engine: &mut Engine) -> bool {
        let Some(text) = engine.poll_voice() else {
            return false;
        };
        self.pending = Some(text);
        true
    }

    /// 丢掉正在录的、在认的与待选的（进私密输入、切会话、关掉开关时调）。
    pub fn cancel(&mut self, engine: &mut Engine) {
        self.stop();
        engine.cancel_voice();
        if self.pending.take().is_some() {
            tracing::debug!("丢了还没上屏的语音文本");
        }
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn model_lookup_comes_from_the_shared_helper() {
        // 真正的用例在 qingjian-voice 的 fetch 里（Server 与设置界面共用那一份）
        let root = std::env::temp_dir().join(format!("qingjian-voice-srv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tier = root.join("voice/base");
        std::fs::create_dir_all(&tier).unwrap();
        std::fs::write(tier.join("base-encoder.int8.onnx"), b"x").unwrap();
        assert_eq!(
            qingjian_voice::fetch::installed(Some(&root), &root, "base"),
            Some(tier)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
