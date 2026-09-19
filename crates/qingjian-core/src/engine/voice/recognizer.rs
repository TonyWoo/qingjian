//! 语音识别器：一段 16 kHz 单声道 f32 样本 → 文本（实现在 `qingjian-voice`，Core 不依赖任何推理库）。

/// 本地语音识别器。产品上按一下开始、再按一下结束、**整段一次性出结果**（不做流式 partial），
/// 所以接口是整段的、不是会话式的。
///
/// 接口是**同步**的：整段前向要几百毫秒，不能放在按键回调里，Engine 把实现放进后台线程
/// （[`VoiceWorker`]）自己调度；这一点跟 [`SentenceScorer`] 同构，而不是 [`Predictor`] 那种
/// 「实现自己保证非阻塞」——识别不联网、不防抖、没有「最新请求优先」的语义。
///
/// [`SentenceScorer`]: crate::sentence::SentenceScorer
/// [`Predictor`]: crate::engine::Predictor
/// [`VoiceWorker`]: super::VoiceWorker
pub trait SpeechRecognizer: Send {
    /// 整段样本 → 文本。样本是 [`VOICE_SAMPLE_RATE`] 赫兹、单声道、f32、范围 `[-1.0, 1.0]`。
    /// 识别不了（模型出错、整段是静音、没人说话）返回 `None`，调用方当这次没有结果。
    ///
    /// [`VOICE_SAMPLE_RATE`]: super::VOICE_SAMPLE_RATE
    fn transcribe(&mut self, samples: &[f32]) -> Option<String>;
}
