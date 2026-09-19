//! 音频的规范形式：16 kHz、单声道、f32。
//!
//! 识别器只接受这个形式，归一化都在这一层做 —— 采样率与声道数是平台属性，
//! 不该让 Core 知道 44.1 kHz 这回事。

mod wav;

#[cfg(feature = "capture")]
mod capture;

pub use wav::read_wav;

#[cfg(feature = "capture")]
pub use capture::Recorder;
