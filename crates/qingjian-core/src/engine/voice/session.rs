//! 一次录音的会话状态：样本缓冲、序号、是否录到上限。

use super::{MAX_VOICE_SECONDS, MIN_VOICE_SECONDS, VOICE_SAMPLE_RATE, VoiceState};

/// 一次录音（从按下开始到按下结束）。壳只在 [`VoiceState::Recording`] 期间喂样本。
#[derive(Debug, Default)]
pub(crate) struct VoiceSession {
    state: VoiceState,

    /// 已经录进来的样本（16 kHz 单声道 f32）。
    samples: Vec<f32>,

    /// 序号：每次开始 / 取消都 +1，识别结果回来时对不上就丢（用户已经重开一段、或切到密码框了）。
    sequence: u64,

    /// 录到上限了，后面的样本被丢掉。
    truncated: bool,
}

impl VoiceSession {
    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// 已经录进来多少秒。
    pub fn recorded_seconds(&self) -> f32 {
        self.samples.len() as f32 / VOICE_SAMPLE_RATE as f32
    }

    /// 是否录到上限、后面的样本被丢了。
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// 开始一次录音：清掉上一次的缓冲，序号 +1 顺带作废在飞的识别结果。
    pub fn begin(&mut self) {
        self.sequence += 1;
        self.samples.clear();
        self.truncated = false;
        self.state = VoiceState::Recording;
    }

    /// 收一段样本；不在录音中返回 `false`。到上限之后只丢新样本、不覆盖已有的
    /// （环形覆盖会把一句话的开头悄悄吃掉，用户看到的文本会莫名其妙地缺开头）。
    pub fn push(&mut self, samples: &[f32]) -> bool {
        if self.state != VoiceState::Recording {
            return false;
        }
        let capacity = MAX_VOICE_SECONDS * VOICE_SAMPLE_RATE as usize;
        let room = capacity.saturating_sub(self.samples.len());
        if samples.len() > room {
            self.samples.extend_from_slice(&samples[..room]);
            self.truncated = true;
        } else {
            self.samples.extend_from_slice(samples);
        }
        true
    }

    /// 结束录音，交出序号与整段样本，转入 [`VoiceState::Transcribing`]。
    /// 太短（误触按键）返回 `None` 并回到 [`VoiceState::Idle`]。
    pub fn finish(&mut self) -> Option<(u64, Vec<f32>)> {
        if self.state != VoiceState::Recording {
            return None;
        }
        if self.recorded_seconds() < MIN_VOICE_SECONDS {
            self.samples.clear();
            self.truncated = false;
            self.state = VoiceState::Idle;
            return None;
        }
        self.truncated = false;
        self.state = VoiceState::Transcribing;
        Some((self.sequence, std::mem::take(&mut self.samples)))
    }

    /// 识别结果已经处理完（不论有没有文本），回到 [`VoiceState::Idle`]。
    pub fn settle(&mut self) {
        self.state = VoiceState::Idle;
    }

    /// 丢掉这一段：序号 +1 让在飞的结果一起作废。Esc、切焦点、进私密输入时调。
    pub fn cancel(&mut self) {
        self.sequence += 1;
        self.samples.clear();
        self.truncated = false;
        self.state = VoiceState::Idle;
    }
}
