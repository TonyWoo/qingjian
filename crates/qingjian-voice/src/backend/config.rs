//! 后端配置：模型目录与推理线程数。

use std::path::PathBuf;

/// 识别后端配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendConfig {
    /// 模型目录：sherpa-onnx 导出的那一份（模型文件 + `tokens.txt`）。
    pub model_dir: PathBuf,

    /// ONNX Runtime 的推理线程数。
    ///
    /// 缺省 **4**，是实测出来的：同一段 6.62 秒音频在 8 核机器上跑 Whisper base，
    /// 2 线程 RTF 0.347、4 线程 0.247、8 线程反而退到 0.384（争抢）。
    /// 解码只在用户按下结束之后发生、那时他正在等，所以占几个核没关系。
    pub threads: i32,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::new(),
            threads: DEFAULT_THREADS,
        }
    }
}

/// 缺省推理线程数，见 [`BackendConfig::threads`]。
pub const DEFAULT_THREADS: i32 = 4;
