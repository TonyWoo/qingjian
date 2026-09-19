//! 跨平台麦克风采集（cpal）：开流、转成规范形式（16 kHz 单声道 f32）、推给调用方。
//!
//! cpal 的 `Stream` **不是 `Send`** —— 必须在创建它的那个线程上 drop。所以这里开一条专用线程，
//! 流的创建、播放、暂停、释放全在那条线程上，外面只拿到一个可 `Send` 的句柄。
//!
//! 采样率策略：**先试 16 kHz 单声道**，成了整条链上一次重采样都不用做；设备不支持才退回
//! 它自己的缺省格式，再用 sherpa-onnx 的 `LinearResampler` 转（sherpa 自己的麦克风示例就这么做）。

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat};
use qingjian_core::VOICE_SAMPLE_RATE;
use sherpa_onnx::LinearResampler;

use crate::error::VoiceError;

/// 采集线程收到的命令。
enum Command {
    Start,
    Stop,
    Quit,
}

/// 麦克风。放着不动不占资源，[`Self::start`] 之后采到的样本由 [`Self::poll`] 取走。
pub struct Recorder {
    commands: Sender<Command>,
    samples: Receiver<Vec<f32>>,
    handle: Option<JoinHandle<()>>,
}

impl Recorder {
    /// 打开麦克风。没有输入设备、设备被占用、系统没给权限都返回 `Err`。
    ///
    /// 流建好之后是暂停的，要 [`Self::start`] 才开始出声。
    pub fn spawn() -> Result<Self, VoiceError> {
        let (commands, command_rx) = channel::<Command>();
        let (samples_tx, samples) = channel::<Vec<f32>>();
        let (ready_tx, ready) = channel::<Result<(), VoiceError>>();
        let handle = std::thread::Builder::new()
            .name("qingjian-voice-capture".to_owned())
            .spawn(move || run(command_rx, samples_tx, ready_tx))
            .map_err(|error| VoiceError::Capture(format!("起不了采集线程：{error}")))?;
        match ready.recv() {
            Ok(Ok(())) => Ok(Self {
                commands,
                samples,
                handle: Some(handle),
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(VoiceError::Capture("采集线程没回话就退出了".to_owned())),
        }
    }

    /// 开始采集。
    pub fn start(&self) {
        let _ = self.commands.send(Command::Start);
    }

    /// 停止采集（流不关，下次还能接着用）。
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    /// 取一段采到的样本（已是 16 kHz 单声道 f32）；没有就 `None`。非阻塞。
    pub fn poll(&self) -> Option<Vec<f32>> {
        self.samples.try_recv().ok()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        // 等它把流释放掉再走：流必须在自己那条线程上 drop，不能由我们代劳
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// 采集线程：建流 → 收命令 → 退出时在同一条线程上释放。
fn run(
    commands: Receiver<Command>,
    samples: Sender<Vec<f32>>,
    ready: Sender<Result<(), VoiceError>>,
) {
    let stream = match build_stream(samples) {
        Ok(stream) => {
            let _ = ready.send(Ok(()));
            stream
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    while let Ok(command) = commands.recv() {
        match command {
            Command::Start => {
                if let Err(error) = stream.play() {
                    tracing::warn!(%error, "麦克风开不了");
                }
            }
            Command::Stop => {
                let _ = stream.pause();
            }
            Command::Quit => break,
        }
    }
}

/// 把设备采集到的数据转成规范形式推给外面。跑在 cpal 的实时回调里。
struct Sink {
    channels: usize,
    resampler: Option<LinearResampler>,
    samples: Sender<Vec<f32>>,
}

impl Sink {
    fn push<T>(&mut self, data: &[T])
    where
        T: Sample,
        f32: FromSample<T>,
    {
        let mono: Vec<f32> = if self.channels <= 1 {
            data.iter()
                .map(|sample| f32::from_sample_(*sample))
                .collect()
        } else {
            // 多声道直接取平均下混：语音场景够用，也不必挑哪个是主声道
            data.chunks_exact(self.channels)
                .map(|frame| {
                    frame
                        .iter()
                        .map(|sample| f32::from_sample_(*sample))
                        .sum::<f32>()
                        / self.channels as f32
                })
                .collect()
        };
        let out = match &self.resampler {
            Some(resampler) => resampler.resample(&mono, false),
            None => mono,
        };
        if !out.is_empty() {
            let _ = self.samples.send(out);
        }
    }
}

/// 打开缺省输入设备并按上面说的采样率策略建流。
fn build_stream(samples: Sender<Vec<f32>>) -> Result<cpal::Stream, VoiceError> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| VoiceError::Capture("这台机器上没有可用的麦克风".to_owned()))?;
    let default = device
        .default_input_config()
        .map_err(|error| VoiceError::Capture(format!("读不到麦克风的缺省格式：{error}")))?;

    let (config, resampler) = match device_accepts_voice_rate(&device) {
        // 设备直接吃 16 kHz：整条链上一次重采样都不用做
        true => (
            cpal::StreamConfig {
                channels: 1,
                sample_rate: VOICE_SAMPLE_RATE,
                buffer_size: cpal::BufferSize::Default,
            },
            None,
        ),
        false => {
            let resampler =
                LinearResampler::create(default.sample_rate() as i32, VOICE_SAMPLE_RATE as i32)
                    .ok_or_else(|| {
                        VoiceError::Capture(format!(
                            "做不出 {} Hz → {VOICE_SAMPLE_RATE} Hz 的重采样器",
                            default.sample_rate()
                        ))
                    })?;
            (default.into(), Some(resampler))
        }
    };

    let mut sink = Sink {
        channels: config.channels as usize,
        resampler,
        samples,
    };
    let errors = |error| tracing::warn!(%error, "麦克风采集出错");
    // 回调本来就要求 `FnMut`，`Sink` 直接移进去改就行，不需要额外加锁
    let stream = match default.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &_| sink.push(data),
            errors,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &_| sink.push(data),
            errors,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            config,
            move |data: &[i32], _: &_| sink.push(data),
            errors,
            None,
        ),
        SampleFormat::I8 => device.build_input_stream(
            config,
            move |data: &[i8], _: &_| sink.push(data),
            errors,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &_| sink.push(data),
            errors,
            None,
        ),
        SampleFormat::U8 => device.build_input_stream(
            config,
            move |data: &[u8], _: &_| sink.push(data),
            errors,
            None,
        ),
        other => {
            return Err(VoiceError::Capture(format!(
                "麦克风的采样格式 {other} 不支持"
            )));
        }
    }
    .map_err(|error| VoiceError::Capture(format!("开不了麦克风：{error}")))?;
    Ok(stream)
}

/// 设备支不支持直接按 16 kHz 单声道开流。
fn device_accepts_voice_rate(device: &cpal::Device) -> bool {
    let Ok(configs) = device.supported_input_configs() else {
        return false;
    };
    configs.into_iter().any(|range| {
        range.channels() == 1
            && range.min_sample_rate() <= VOICE_SAMPLE_RATE
            && range.max_sample_rate() >= VOICE_SAMPLE_RATE
    })
}
