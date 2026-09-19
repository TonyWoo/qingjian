//! 本地离线语音识别：给 Core 的 [`SpeechRecognizer`] 提供实现，同时提供音频读入。
//!
//! 全程离线：模型是本机文件，不联网、不发任何请求。
//! 采样格式统一到 16 kHz 单声道 f32（[`qingjian_core::VOICE_SAMPLE_RATE`]），
//! 识别器只接受这个形式，读入侧负责归一化到它。
//!
//! feature：[`sherpa`] 打开真正的识别后端，缺省关（见 `Cargo.toml` 里的说明）。

pub mod audio;
mod backend;
mod error;
pub mod fetch;

pub use backend::{BackendConfig, DEFAULT_THREADS, load_recognizer};
pub use error::VoiceError;
