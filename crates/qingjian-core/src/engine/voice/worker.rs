//! 后台识别线程：整段模型前向要几百毫秒，不能放在按键回调里。

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use super::{SpeechRecognizer, VOICE_SAMPLE_RATE};

/// 一次识别任务。
struct Job {
    sequence: u64,
    samples: Vec<f32>,
}

/// 识别好的文本，与任务一一对应。
pub(crate) struct Transcribed {
    pub sequence: u64,

    /// `None` 表示识别器没给出结果（静音 / 出错）。
    pub text: Option<String>,
}

/// 后台识别线程：整段前向要几百毫秒，不能放在按键回调里。线程随本结构一起结束。
///
/// 与 `RescoreWorker` 的区别：**样本一条都不能丢**。重打分可以把排队时攒下的旧任务扔掉
/// （旧任务对应已经过去的输入状态），录音的每一帧都有意义，所以这里只转发、不挑选。
pub(crate) struct VoiceWorker {
    jobs: Sender<Job>,
    results: Receiver<Transcribed>,
    handle: Option<JoinHandle<()>>,
}

impl VoiceWorker {
    pub fn spawn(mut recognizer: Box<dyn SpeechRecognizer>) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (result_tx, results) = channel::<Transcribed>();
        let handle = std::thread::Builder::new()
            .name("qingjian-voice".to_owned())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let seconds = job.samples.len() as f32 / VOICE_SAMPLE_RATE as f32;
                    let started = std::time::Instant::now();
                    let text = recognizer.transcribe(&job.samples);
                    let ms = started.elapsed().as_millis();
                    tracing::debug!(
                        seconds,
                        ms,
                        rtf = ms as f64 / (seconds as f64 * 1000.0),
                        "语音识别完成"
                    );
                    let done = Transcribed {
                        sequence: job.sequence,
                        text,
                    };
                    if result_tx.send(done).is_err() {
                        break;
                    }
                }
            })
            .ok();
        if handle.is_none() {
            tracing::warn!("起不了语音识别线程，本次不用语音输入");
        }
        Self {
            jobs,
            results,
            handle,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.handle.is_some()
    }

    pub fn submit(&self, sequence: u64, samples: Vec<f32>) {
        if self.jobs.send(Job { sequence, samples }).is_err() {
            tracing::warn!("语音识别线程已退出");
        }
    }

    /// 取一条识别结果；没有就 `None`。
    pub fn poll(&self) -> Option<Transcribed> {
        self.results.try_recv().ok()
    }
}

impl Drop for VoiceWorker {
    fn drop(&mut self) {
        // 关掉任务通道线程就会退出；不等它（模型可能正算到一半）
        let _ = self.handle.take();
    }
}
