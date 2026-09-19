//! Router 上的语音输入：接 / 卸识别器、处理触发键、每个 tick 推进录音与识别。

use std::path::PathBuf;

use qingjian_core::VoiceState;
use qingjian_platform::VoiceConfig;
use qingjian_platform::protocol::{KeyEvent, KeyModifiers};

use crate::dispatch::Router;

impl Router {
    /// 修饰键比物理组合（与 [`Self::matches_translate_combo`] 同款）。
    pub(crate) fn matches_voice_combo(&self, event: &KeyEvent) -> bool {
        let combo = self.config.voice_combo;
        event.character == Some(combo.key)
            && event.modifiers.chord() == KeyModifiers::from(combo.modifiers)
    }

    /// 启动时接一次。把查找根目录记下来：设置程序是**另一个进程**，它把模型下好之后
    /// Server 不会自己发现，只能等配置热加载触发时重扫（见 [`Self::apply_voice_config`]）。
    pub fn configure_voice(
        &mut self,
        user_dir: Option<PathBuf>,
        bundled_root: PathBuf,
        config: &VoiceConfig,
    ) {
        self.voice_roots = Some((user_dir, bundled_root));
        self.applied_voice = config.clone();
        self.apply_voice();
    }

    /// `[voice]` 变了才重接 / 卸（开关翻转、换档位）。**每次都重扫目录** ——
    /// 设置程序下完模型会写一个配置键触发 mtime 变化，这里就是接住那个变化的点。
    pub fn apply_voice_config(&mut self, config: &VoiceConfig) {
        if *config == self.applied_voice {
            return;
        }
        self.applied_voice = config.clone();
        self.apply_voice();
    }

    fn apply_voice(&mut self) {
        if !self.applied_voice.enabled {
            self.teardown_voice();
            return;
        }
        let Some((user_dir, bundled_root)) = self.voice_roots.clone() else {
            return;
        };
        let model = super::find_model(user_dir.as_deref(), &bundled_root, &self.applied_voice.tier);
        let Some(dir) = model else {
            tracing::warn!("[voice] 开着但没找到语音模型，语音输入用不了");
            self.teardown_voice();
            return;
        };
        super::attach(&mut self.engine, &dir, qingjian_voice::DEFAULT_THREADS);
    }

    /// 卸掉识别器并把录到一半、还没上屏的一并作废。
    fn teardown_voice(&mut self) {
        if self.engine.voice_state() != VoiceState::Idle || self.voice.has_pending() {
            self.voice.cancel(&mut self.engine);
        }
        super::detach(&mut self.engine);
    }

    /// 语音触发键：没在录就开始，在录就停。返回是否吃掉这个键。
    ///
    /// 模型没接、开关没开、麦克风打不开、或正在组句时返回 `false` —— 都当普通键放过去，
    /// 不抢断用户的输入。
    pub(crate) fn toggle_voice(&mut self) -> bool {
        if !self.applied_voice.enabled {
            return false;
        }
        // 第一次按才开麦克风；开不了（没设备 / 没权限）就别吃这个键
        if !self.voice.ready() && !self.voice.ensure_recorder() {
            return false;
        }
        match self.engine.voice_state() {
            VoiceState::Idle => {
                if !self.engine.start_voice() {
                    // 正在组句：不抢断，那截拼音算上屏还是丢弃该由用户决定
                    tracing::debug!("正在组句，语音触发键先不管");
                    return false;
                }
                self.voice.start();
                tracing::info!("开始录音");
                true
            }
            VoiceState::Recording => {
                self.voice.stop();
                self.engine.stop_voice();
                tracing::info!("录音结束，正在识别");
                true
            }
            // 正在认：吃掉这个键，但不做任何事（免得它变成打字）
            VoiceState::Transcribing => true,
        }
    }

    /// 每个 tick：把采到的样本喂进去、看识别结果到了没有。
    pub(crate) fn advance_voice(&mut self) {
        if self.engine.voice_state() == VoiceState::Recording {
            let seconds = self.voice.drain_into(&mut self.engine);
            if seconds > 0.0 {
                tracing::trace!(seconds, "语音样本已喂入");
            }
        }
        if self.voice.poll(&mut self.engine) {
            tracing::info!("语音识别完成，等下一次按键上屏");
        }
    }
}
