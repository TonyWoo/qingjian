//! WAV 文件读取：读成规范形式的样本（16 kHz 单声道 f32）。
//!
//! 只收 16 kHz：sherpa-onnx 随模型发的测试音频就是这个格式。别的采样率要重采样，
//! 等上麦克风采集时一起做（那时才需要真正的重采样器），现在直接报错比悄悄用错采样率好。

use std::path::Path;

use qingjian_core::VOICE_SAMPLE_RATE;

use crate::error::VoiceError;

/// 读一个 WAV，出来是 16 kHz 单声道 f32；多声道直接取平均下混。
pub fn read_wav(path: &Path) -> Result<Vec<f32>, VoiceError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != VOICE_SAMPLE_RATE {
        return Err(VoiceError::UnsupportedFormat {
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        });
    }

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            // 满量程整数映到 [-1.0, 1.0)：16 位就是 32768
            let peak = (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / peak))
                .collect::<Result<_, _>>()?
        }
    };

    Ok(downmix(&interleaved, spec.channels as usize))
}

/// 交错采样下混成单声道（取平均）。末尾不足一帧的零头丢掉。
fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}
