//! 语音输入：Engine 这一侧的录音状态机、识别 worker 与上屏。
//!
//! 音频采集在平台层（麦克风是和键盘同级的系统输入设备，把系统输入事件翻译成 Core 的输入正是
//! 平台层的职责），识别在 `qingjian-voice` 里，这里只做三件事：接住平台层喂进来的样本、
//! 把整段交给后台线程、把回来的文本上屏。
//!
//! 采样格式统一到 [`VOICE_SAMPLE_RATE`]：采集与 WAV 读入都重采样到它，识别器只接受它。
//! 归一化放在采集侧而不是这里 —— 采样率与声道数是平台属性，Core 一旦知道 44.1 kHz 这回事，
//! 换平台就可能要动 Core 的代码。

mod commit;
mod recognizer;
mod session;
mod state;
mod worker;

pub use recognizer::SpeechRecognizer;
pub(crate) use session::VoiceSession;
pub use state::VoiceState;
pub(crate) use worker::VoiceWorker;

use super::Engine;

/// 语音输入的规范采样率。整条链上只有这一个数：采集与 WAV 读入都重采样到它，识别器只接受它。
/// 16 kHz 是所有 sherpa-onnx 模型的输入要求。
pub const VOICE_SAMPLE_RATE: u32 = 16_000;

/// 单次录音最长多少秒：到顶后只丢新样本，不覆盖已有的（环形覆盖会把一句话的开头悄悄吃掉）。
pub const MAX_VOICE_SECONDS: usize = 60;

/// 一段录音至少多少秒才值得送去识别：挡掉误触按键时的空转。
pub const MIN_VOICE_SECONDS: f32 = 0.3;

impl Engine {
    /// 挂上语音识别器。缺省没有：模型是几百 MB 的重资产、构造可能失败，用 `Option` 比空实现诚实。
    pub fn with_speech_recognizer(mut self, recognizer: Box<dyn SpeechRecognizer>) -> Self {
        self.set_speech_recognizer(Some(recognizer));
        self
    }

    /// 运行时换 / 卸语音识别器（壳里模型在后台加载完才接上，配置关掉就卸）。正在录或正在认的一并作废。
    pub fn set_speech_recognizer(&mut self, recognizer: Option<Box<dyn SpeechRecognizer>>) {
        self.voice_session.cancel();
        self.voice = recognizer.map(VoiceWorker::spawn);
    }

    /// 语音输入现在能不能用：识别器活着，且不在私密输入里。壳据此决定要不要显示语音入口。
    pub fn voice_available(&self) -> bool {
        self.voice.as_ref().is_some_and(VoiceWorker::is_alive) && !self.private
    }

    /// 当前阶段，壳拿它画指示器。
    pub fn voice_state(&self) -> VoiceState {
        self.voice_session.state()
    }

    /// 已经录进来多少秒，壳拿它显示时长、接近上限时提示。
    pub fn voice_recorded_seconds(&self) -> f32 {
        self.voice_session.recorded_seconds()
    }

    /// 是否已录到上限、后面的样本被丢了。
    pub fn voice_truncated(&self) -> bool {
        self.voice_session.truncated()
    }

    /// 开始一次录音（toggle 第一下）。识别器没接、在私密输入里、或还在组句时返回 `false`
    /// —— 组句中途不抢断，那截拼音算上屏还是丢弃该由壳决定。
    pub fn start_voice(&mut self) -> bool {
        if !self.voice_available() || !self.composition.is_empty() {
            return false;
        }
        self.voice_session.begin();
        true
    }

    /// 喂一段采集到的样本（[`VOICE_SAMPLE_RATE`] 赫兹、单声道、f32）；不在录音中返回 `false`。
    pub fn push_voice_samples(&mut self, samples: &[f32]) -> bool {
        self.voice_session.push(samples)
    }

    /// 结束录音（toggle 第二下），整段交给后台识别，结果由 [`Self::poll_voice`] 取。
    /// 没在录、或录得太短（误触）返回 `false`。
    pub fn stop_voice(&mut self) -> bool {
        let Some((sequence, samples)) = self.voice_session.finish() else {
            return false;
        };
        let Some(voice) = &self.voice else {
            return false;
        };
        voice.submit(sequence, samples);
        true
    }

    /// 丢掉这一段：Esc、切焦点、进私密输入时调。在飞的识别结果也会一起作废。
    pub fn cancel_voice(&mut self) {
        self.voice_session.cancel();
    }

    /// 取识别结果；非阻塞，壳在定时器里调。拿到就地走 [`Self::commit_voice`] 上屏，
    /// 返回**要塞进应用的最终文本**（繁体模式下已经是繁体），没有结果返回 `None`。
    ///
    /// 这里不把「取结果」和「上屏」拆成两步：语音没有「要不要接受」这一步，
    /// 拆开只会给每个壳留一次「忘了调上屏，于是学习与日志整条断掉」的机会。
    pub fn poll_voice(&mut self) -> Option<String> {
        let transcribed = self.voice.as_ref()?.poll()?;
        if transcribed.sequence != self.voice_session.sequence() {
            // 用户已经重开了一段、或者切到密码框了，这条作废
            tracing::debug!(sequence = transcribed.sequence, "语音结果过期，丢弃");
            return None;
        }
        self.voice_session.settle();
        let text = transcribed.text?;
        Some(self.commit_voice(&text))
    }
}
