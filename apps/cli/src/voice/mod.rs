//! 语音的 CLI 通道：跑一个 WAV 看识别结果、按目录算 CER 与 RTF。
//!
//! 两条路都刻意走 Core 的完整语音状态机（开始 → 喂样本 → 结束 → 轮询），而不是直接调识别器 ——
//! 这样 CLI 就是第二个壳，平台壳将来要走的路径在第一步就被验掉了。

mod cer;
mod pair;
mod report;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use qingjian_core::{Engine, VOICE_SAMPLE_RATE, VoiceState};
use qingjian_voice::fetch::{Download, FetchEvent};

use crate::error::CliError;
use report::{Report, Row};

/// 等识别结果的上限：本地模型再慢也不该超过这个数。
const WAIT: Duration = Duration::from_secs(120);

/// 一个 WAV 走一遍完整语音输入，返回要打印的报告。
pub fn run_wav(engine: &mut Engine, path: &Path) -> Result<String, CliError> {
    if !engine.voice_available() {
        return Err(CliError::VoiceModelMissing);
    }

    let read_started = Instant::now();
    let samples = qingjian_voice::audio::read_wav(path)?;
    let read = read_started.elapsed();
    let seconds = samples.len() as f32 / VOICE_SAMPLE_RATE as f32;

    let mut out = String::new();
    writeln!(out, "> {}", path.display()).ok();
    writeln!(
        out,
        "音频 {seconds:.2} s（{} 样本 @ {VOICE_SAMPLE_RATE} Hz 单声道）  读入 {} ms",
        samples.len(),
        read.as_millis()
    )
    .ok();

    engine.start_voice();
    engine.push_voice_samples(&samples);
    let started = Instant::now();
    if !engine.stop_voice() {
        writeln!(out, "录音短于 0.3 s，没有送去识别").ok();
        return Ok(out);
    }
    // 走一遍完整链路（含上屏：学习 / 输入日志），单文件这条就是拿来验这个的
    let text = wait(engine)?.map(|text| engine.accept_voice(&text));
    let elapsed = started.elapsed();

    writeln!(
        out,
        "识别 {} ms  RTF {:.3}",
        elapsed.as_millis(),
        report::rtf(elapsed.as_millis(), seconds)
    )
    .ok();
    writeln!(out, "文本 {}", text.as_deref().unwrap_or("（空）")).ok();
    Ok(out)
}

/// 按目录算 CER 与 RTF。目录里每对 `<名字>.wav` + `<名字>.txt` 算一条。
pub fn run_eval(engine: &mut Engine, dirs: &[PathBuf], misses: usize) -> Result<String, CliError> {
    if !engine.voice_available() {
        return Err(CliError::VoiceModelMissing);
    }
    let mut report = Report::default();
    for dir in dirs {
        let (pairs, missing) = pair::scan(dir)?;
        report.missing_reference += missing;
        for pair in pairs {
            // 一条读不了（采样率不对、文件坏了）只跳过并计数，不该中断整场评测
            let Ok(samples) = qingjian_voice::audio::read_wav(&pair.wav) else {
                tracing::warn!(wav = %pair.wav.display(), "音频读不了，这条跳过");
                report.skipped += 1;
                continue;
            };
            let seconds = samples.len() as f32 / VOICE_SAMPLE_RATE as f32;

            engine.start_voice();
            engine.push_voice_samples(&samples);
            let started = Instant::now();
            let recognized = engine.stop_voice();
            // 只取不上屏：`accept_voice` 会记输入日志与个人 n-gram，拿评测集跑一遍等于把这些
            // 音频的文本学进用户词典，CER 也不该受这条路影响
            let hypothesis = if recognized { wait(engine)? } else { None };
            let millis = started.elapsed().as_millis();

            let (distance, reference_chars) =
                cer::cer(&pair.reference, hypothesis.as_deref().unwrap_or(""));
            report.rows.push(Row {
                name: pair
                    .wav
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?")
                    .to_owned(),
                reference: pair.reference,
                hypothesis,
                distance,
                reference_chars,
                seconds,
                millis,
            });
        }
    }
    Ok(report.render(misses))
}

/// 下一档语音模型，返回要打印的报告。进度走 stderr，不混进 stdout 的报告里。
pub fn run_fetch(name: &str, dir: Option<PathBuf>) -> Result<String, CliError> {
    let Some(tier) = qingjian_voice::fetch::tier(name) else {
        let available: Vec<&str> = qingjian_voice::fetch::tiers()
            .map(|(name, _)| name)
            .collect();
        return Err(CliError::VoiceUnknownTier {
            name: name.to_owned(),
            available: available.join(", "),
        });
    };
    let dir = dir
        .unwrap_or_else(|| PathBuf::from("data/voice"))
        .join(name);

    let mut out = String::new();
    writeln!(out, "档位 {name}（{}）{}", tier.label, tier.size_text()).ok();
    writeln!(out, "{}", tier.note).ok();
    writeln!(out, "下到 {}", dir.display()).ok();

    let download = Download::spawn(tier, dir);
    let started = Instant::now();
    // 每 5% 报一次，免得刷屏
    let mut last = u64::MAX;
    loop {
        match download.poll() {
            Some(FetchEvent::Progress { done, total }) => {
                let percent = done.saturating_mul(100).checked_div(total).unwrap_or(100);
                if last == u64::MAX || percent / 5 != last / 5 {
                    last = percent;
                    eprintln!("  下载中 {percent}%（{done} / {total} 字节）");
                }
            }
            Some(FetchEvent::Done { dir }) => {
                writeln!(
                    out,
                    "完成，用时 {:.1} s，模型在 {}",
                    started.elapsed().as_secs_f32(),
                    dir.display()
                )
                .ok();
                return Ok(out);
            }
            Some(FetchEvent::Failed(error)) => return Err(CliError::Voice(error)),
            None if download.is_finished() => return Err(CliError::VoiceFetchStopped),
            None => {}
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 等识别结果。CLI 没有壳的定时器，就阻塞轮询到结果为止。
/// 返回 `None` 表示结果到了但没有文本（静音 / 识别不出）。
fn wait(engine: &mut Engine) -> Result<Option<String>, CliError> {
    let started = Instant::now();
    loop {
        if let Some(text) = engine.poll_voice() {
            return Ok(Some(text));
        }
        // 结果到了（哪怕是空的）状态就回到 Idle，据此把「还没到」与「到了但是空的」分开
        if engine.voice_state() == VoiceState::Idle {
            return Ok(None);
        }
        if started.elapsed() > WAIT {
            return Err(CliError::VoiceTimeout);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
