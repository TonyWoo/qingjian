//! 语音模块的错误：后端没编译进来、模型加载失败、音频读不了。

use std::path::PathBuf;

/// 语音模块的错误。
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    /// 这个构建没编译 `sherpa` feature，认不了模型。调用方不需要写 `#[cfg]`，真用了才拿到这个。
    #[error("this build has no speech backend compiled in (enable the `sherpa` feature)")]
    BackendNotCompiled,

    /// 模型加载失败（目录里没有模型文件、文件读不动、格式不对）。
    #[error("failed to load the speech model from {path}")]
    LoadModel { path: PathBuf },

    /// WAV 文件读不动。
    #[error("failed to read the audio file: {0}")]
    Wav(#[from] hound::Error),

    /// 麦克风打不开或采集中断（没有输入设备、被占用、系统没给权限、采样格式不支持）。
    #[error("failed to capture from the microphone: {0}")]
    Capture(String),

    /// 下载模型时出错（发请求、读响应体、校验不过）。消息里带着是哪个文件、为什么。
    #[error("failed to fetch the speech model: {0}")]
    Fetch(String),

    /// 这次下载被取消了。临时目录已经清掉，正式目录没动。
    #[error("the download was cancelled")]
    Cancelled,

    /// 把下好的模型写进磁盘失败（建目录、写文件、改名）。
    #[error("failed to store the speech model: {0}")]
    Store(#[from] std::io::Error),

    /// 音频格式不是识别器要的规范形式。
    #[error(
        "expected {VOICE_SAMPLE_RATE} Hz audio, got {sample_rate} Hz with {channels} channel(s)",
        VOICE_SAMPLE_RATE = qingjian_core::VOICE_SAMPLE_RATE
    )]
    UnsupportedFormat { sample_rate: u32, channels: u16 },
}
