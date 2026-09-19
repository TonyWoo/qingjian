//! 录音状态：没在录 / 正在录 / 正在认。壳拿它画指示器、决定要不要起轮询定时器。

/// 一次语音输入所处的阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceState {
    /// 没在录。
    #[default]
    Idle,

    /// 正在录：壳该把采集到的样本喂给 [`Engine::push_voice_samples`]。
    ///
    /// [`Engine::push_voice_samples`]: crate::Engine::push_voice_samples
    Recording,

    /// 已经交给后台识别：壳该起定时器轮询 [`Engine::poll_voice`]。
    ///
    /// [`Engine::poll_voice`]: crate::Engine::poll_voice
    Transcribing,
}
