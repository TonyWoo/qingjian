//! sherpa-onnx 后端：整段 16 kHz 单声道 f32 进去、文本出来。

use std::path::Path;

use qingjian_core::{SpeechRecognizer, VOICE_SAMPLE_RATE};
use sherpa_onnx::{
    OfflineModelConfig, OfflineParaformerModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineSenseVoiceModelConfig, OfflineTransducerModelConfig, OfflineWhisperModelConfig,
};

use crate::backend::BackendConfig;
use crate::error::VoiceError;

/// sherpa-onnx 的离线识别器。
///
/// 它是 `Send + Sync` 的（crate 里已经 `unsafe impl` 过，注明 C 库单对象使用线程安全），
/// 所以可以直接交给 Core 的后台线程，这里不需要自己写 `unsafe impl`。
///
/// 用的是**离线**识别器 `OfflineRecognizer`（整段进、整段出），与 Core 那个整段式的
/// [`SpeechRecognizer`] 正好对上。sherpa-onnx 的 `streaming-zipformer-*` 是**在线**模型，
/// 走的是 `OnlineRecognizer` 那条会话式接口，这里用不了。
pub struct SherpaRecognizer {
    recognizer: OfflineRecognizer,
}

impl SherpaRecognizer {
    /// 从模型目录加载。是哪个模型家族按目录里的文件名认，见 [`detect`]。
    pub fn load(config: &BackendConfig) -> Result<Self, VoiceError> {
        let path = config.model_dir.clone();
        let model_config = detect(&config.model_dir, config.threads)?;
        let recognizer = OfflineRecognizer::create(&OfflineRecognizerConfig {
            model_config,
            ..Default::default()
        })
        .ok_or(VoiceError::LoadModel { path })?;
        Ok(Self { recognizer })
    }
}

impl SpeechRecognizer for SherpaRecognizer {
    fn transcribe(&mut self, samples: &[f32]) -> Option<String> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(VOICE_SAMPLE_RATE as i32, samples);
        self.recognizer.decode(&stream);
        let text = stream.get_result()?.text.trim().to_owned();
        (!text.is_empty()).then_some(text)
    }
}

/// 按目录里的文件名认模型家族。sherpa-onnx 各家族导出的文件是固定的：
/// transducer 有 encoder / decoder / joiner 三件套，Whisper 只有 encoder / decoder，
/// SenseVoice 与 Paraformer 都只有一个 `model.onnx` —— 后两者只能靠目录名分。
fn detect(dir: &Path, threads: i32) -> Result<OfflineModelConfig, VoiceError> {
    let files = read_dir_names(dir)?;

    let mut config = OfflineModelConfig {
        // 文件名不固定：SenseVoice 与多数模型叫 `tokens.txt`，Whisper 的包叫 `base-tokens.txt`
        // 这类带模型名前缀的。按后缀找，找不到再退回家喻户晓的那个名字。
        tokens: Some(path_text(
            &dir.join(
                files
                    .iter()
                    .find(|name| name.ends_with("tokens.txt"))
                    .map(String::as_str)
                    .unwrap_or("tokens.txt"),
            ),
        )),
        num_threads: threads,
        provider: Some("cpu".to_owned()),
        ..Default::default()
    };
    // 文件名匹配用「包含」而不是「开头」：Whisper 的文件带模型名前缀（`tiny.en-encoder.int8.onnx`），
    // 而 transducer 的是 `encoder-epoch-99-avg-1.int8.onnx`。
    // 同名会有 fp32 与 int8 两份（SenseVoice 的包就是 `model.onnx` 894 MB 加 `model.int8.onnx` 228 MB），
    // 优先取 int8：快得多、内存小得多，准确率差别在这个场景下可以忽略。
    // `read_dir` 的顺序不保证，所以必须显式排，不能指望「第一个就是 int8」。
    let pick = |needle: &str| {
        let mut matches: Vec<&String> = files
            .iter()
            .filter(|name| name.contains(needle) && name.ends_with(".onnx"))
            .collect();
        matches.sort_by_key(|name| !name.contains(".int8."));
        matches.first().map(|name| path_text(&dir.join(name)))
    };

    match (pick("encoder"), pick("decoder"), pick("joiner")) {
        (Some(encoder), Some(decoder), Some(joiner)) => {
            config.transducer = OfflineTransducerModelConfig {
                encoder: Some(encoder),
                decoder: Some(decoder),
                joiner: Some(joiner),
            };
        }
        (Some(encoder), Some(decoder), None) => {
            config.whisper = OfflineWhisperModelConfig {
                encoder: Some(encoder),
                decoder: Some(decoder),
                // 中英混说：Whisper 不给语种就是自动判别，写死会让夹英文的句子被硬当中文
                language: None,
                task: Some("transcribe".to_owned()),
                ..Default::default()
            };
        }
        _ => {
            let Some(model) = pick("model") else {
                return Err(VoiceError::LoadModel {
                    path: dir.to_path_buf(),
                });
            };
            if dir_name_contains(dir, "paraformer") {
                config.paraformer = OfflineParaformerModelConfig { model: Some(model) };
            } else {
                config.sense_voice = OfflineSenseVoiceModelConfig {
                    model: Some(model),
                    language: Some("auto".to_owned()),
                    // 带标点：语音是直接上屏的，没有候选窗可改，标点只能靠模型出
                    use_itn: true,
                };
            }
        }
    }
    Ok(config)
}

fn read_dir_names(dir: &Path) -> Result<Vec<String>, VoiceError> {
    let entries = std::fs::read_dir(dir).map_err(|_| VoiceError::LoadModel {
        path: dir.to_path_buf(),
    })?;
    Ok(entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect())
}

fn dir_name_contains(dir: &Path, needle: &str) -> bool {
    dir.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().contains(needle))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
