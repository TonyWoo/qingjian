//! 识别后端：把 Core 的 [`SpeechRecognizer`] 接到具体模型上。

mod config;

#[cfg(feature = "sherpa")]
mod sherpa;

pub use config::{BackendConfig, DEFAULT_THREADS};

use qingjian_core::SpeechRecognizer;

use crate::error::VoiceError;

/// 按配置造一个识别器。
///
/// 没编译 `sherpa` feature 时永远返回 [`VoiceError::BackendNotCompiled`] —— 把 `#[cfg]` 全部收敛在
/// 这个文件里，CLI 与各平台壳因此不用写任何条件编译，`--voice-model` 照常出现在 `--help` 里。
#[cfg(feature = "sherpa")]
pub fn load_recognizer(config: &BackendConfig) -> Result<Box<dyn SpeechRecognizer>, VoiceError> {
    Ok(Box::new(sherpa::SherpaRecognizer::load(config)?))
}

#[cfg(not(feature = "sherpa"))]
pub fn load_recognizer(_config: &BackendConfig) -> Result<Box<dyn SpeechRecognizer>, VoiceError> {
    Err(VoiceError::BackendNotCompiled)
}
