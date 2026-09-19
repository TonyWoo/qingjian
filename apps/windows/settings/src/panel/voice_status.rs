//! 语音页「下载模型」的状态，挂在 [`Settings`](super::Settings) 上，由根组件的 `update` 推进、语音页显示。

/// 模型下载的状态。
#[derive(Clone)]
pub(crate) enum VoiceStatus {
    /// 没在下。
    Idle,

    /// 正在下（带要下的档位名）。
    Downloading(String),

    /// 下好了（带说明）。
    Ok(String),

    /// 失败（带错误）。
    Failed(String),
}
