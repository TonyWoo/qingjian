//! Router 上的语音输入：接 / 卸识别器、处理触发键、每个 tick 推进录音与识别。

use std::path::PathBuf;

use qingjian_core::{MAX_VOICE_SECONDS, VoiceState};
use qingjian_platform::VoiceConfig;
use qingjian_platform::protocol::{KeyEvent, KeyModifiers};

use super::VoiceChoice;
use crate::dispatch::Router;
use crate::dispatch::key::codes;

impl Router {
    /// 修饰键比物理组合（与 [`Self::matches_translate_combo`] 同款）。
    pub(crate) fn matches_voice_combo(&self, event: &KeyEvent) -> bool {
        let combo = self.config.voice_combo;
        event.character == Some(combo.key)
            && event.modifiers.chord() == KeyModifiers::from(combo.modifiers)
    }

    /// 语音候选摆在候选窗里时，这个键算接受还是丢弃。两个都不是返回 `None`，
    /// 调用方的意思是「用户不要这段了」。
    ///
    /// 空格与 `1` 都接受：候选窗里它排第 1 位，空格是选候选的老习惯。带 Ctrl / Alt / Win 的组合
    /// 一律不算（那是应用的快捷键，别在语音上蹭）。DLL 那边拦的是同一组键（`would_eat`）。
    pub(crate) fn voice_choice(&self, event: &KeyEvent) -> Option<VoiceChoice> {
        if event.modifiers.has_command_key() {
            return None;
        }
        match event.virtual_key {
            codes::ESCAPE => Some(VoiceChoice::Discard),
            codes::SPACE => Some(VoiceChoice::Accept),
            _ if codes::digit(event) == Some(1) => Some(VoiceChoice::Accept),
            _ => None,
        }
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
        let model = qingjian_voice::fetch::installed(
            user_dir.as_deref(),
            &bundled_root,
            &self.applied_voice.tier,
        );
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
    /// 模型没接、开关没开、麦克风打不开、或**要开始**时正在组句，都返回 `false` ——
    /// 当普通键放过去，不抢断用户的输入。**停止不看组句**：录着的时候用户多半正敲着拼音，
    /// 那会儿按第二下必须停得下来（`message.rs` 的调用点原来在外层挡了一道，已去掉）。
    pub(crate) fn toggle_voice(&mut self) -> bool {
        if !self.applied_voice.enabled {
            return false;
        }
        match self.engine.voice_state() {
            VoiceState::Recording => {
                self.finish_recording();
                true
            }
            // 正在认：吃掉这个键，但不做任何事（免得它变成打字）
            VoiceState::Transcribing => true,
            VoiceState::Idle => {
                if !self.engine.composition().is_empty() {
                    // 组句中途不抢断：那截拼音算上屏还是丢弃该由用户决定
                    tracing::debug!("正在组句，语音触发键先不管");
                    return false;
                }
                // 上一段的结果还摆在候选窗里就再按触发键：当用户不要它了，重录
                self.voice.discard();
                // 第一次按才开麦克风；开不了（没设备 / 没权限）就别吃这个键
                if !self.voice.ready() && !self.voice.ensure_recorder() {
                    return false;
                }
                if !self.engine.start_voice() {
                    tracing::debug!("起不了录音（没接识别器或在私密输入里），触发键放行");
                    return false;
                }
                self.voice.start();
                tracing::info!("开始录音");
                self.reconcile_voice_status();
                true
            }
        }
    }

    /// 停录并把这一整段交给后台识别。触发键第二下与录到上限都走这里。
    fn finish_recording(&mut self) {
        // 先量后停：stop_voice 会把缓冲交出去，交出去就读不到了
        let seconds = self.engine.voice_recorded_seconds();
        let truncated = self.engine.voice_truncated();
        self.voice.stop();
        if self.engine.stop_voice() {
            tracing::info!(seconds, truncated, "录音结束，正在识别");
        } else {
            // 不到 MIN_VOICE_SECONDS 就丢：误触按键，或者**麦克风一个样本都没给**。
            // 后者是采集没通时唯一的信号，所以把秒数打出来 —— 0.0 就是没采到。
            tracing::warn!(seconds, "录到的太短，不送去识别");
        }
        self.reconcile_voice_status();
    }

    /// 每个 tick：把采到的样本喂进去、到上限就收工、看识别结果到了没有。
    pub(crate) fn advance_voice(&mut self) {
        if self.engine.voice_state() == VoiceState::Recording {
            let seconds = self.voice.drain_into(&mut self.engine);
            if seconds > 0.0 {
                tracing::trace!(seconds, "语音样本已喂入");
            }
            // 到上限就地收工：再录下去样本只会被丢掉（不覆盖已有的），让用户白说一整段更糟
            if self.engine.voice_truncated() {
                tracing::info!(limit = MAX_VOICE_SECONDS, "录到上限，自动停止");
                self.finish_recording();
            }
        }
        if self.voice.poll(&mut self.engine) {
            tracing::info!("语音识别完成，等下一次按键上屏");
        }
        // 状态条上的语音格：识别完成、被按键捎走这些都在别处发生，每个 tick 对一次账最省事
        self.reconcile_voice_status();
    }
}
