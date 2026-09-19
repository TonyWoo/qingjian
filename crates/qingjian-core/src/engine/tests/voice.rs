//! 语音输入：录音状态机、序号作废、私密输入与上屏日志。
//!
//! 用一个假的 [`SpeechRecognizer`] 覆盖状态机，不碰任何真实模型 —— 与 Core「缺省全是 No*
//! 空实现、单测不带真实数据也能跑」的做法一致。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::*;

/// 假识别器：把样本数当成识别结果，并记下跑过几次，让测试能等它真的跑完而不靠 sleep 撞运气。
struct FakeRecognizer {
    runs: Arc<AtomicUsize>,
}

impl SpeechRecognizer for FakeRecognizer {
    fn transcribe(&mut self, samples: &[f32]) -> Option<String> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Some(samples.len().to_string())
    }
}

/// 假识别器：永远识别不出东西（静音）。
struct SilentRecognizer;

impl SpeechRecognizer for SilentRecognizer {
    fn transcribe(&mut self, _samples: &[f32]) -> Option<String> {
        None
    }
}

fn voice_engine() -> (Engine, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let engine = engine().with_speech_recognizer(Box::new(FakeRecognizer { runs: runs.clone() }));
    (engine, runs)
}

/// 录 `seconds` 秒的静音喂进去。
fn feed(engine: &mut Engine, seconds: f32) {
    let samples = vec![0.0f32; (seconds * VOICE_SAMPLE_RATE as f32) as usize];
    assert!(engine.push_voice_samples(&samples));
}

/// 等后台线程把结果算完（通道里已经有了）。
fn wait_runs(runs: &AtomicUsize, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while runs.load(Ordering::SeqCst) < expected {
        assert!(Instant::now() < deadline, "识别线程没有跑");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 轮询到结果；识别不出返回 `None`。
fn wait_for_text(engine: &mut Engine) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(text) = engine.poll_voice() {
            return Some(text);
        }
        if engine.voice_state() == VoiceState::Idle {
            return None;
        }
        assert!(Instant::now() < deadline, "识别线程没有回结果");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn voice_is_unavailable_without_a_recognizer() {
    let mut engine = engine();
    assert!(!engine.voice_available());
    assert!(!engine.start_voice());
    assert_eq!(engine.voice_state(), VoiceState::Idle);
}

#[test]
fn a_recording_round_trip_ends_in_the_recognized_text() {
    let (mut engine, _runs) = voice_engine();
    assert!(engine.voice_available());

    assert!(engine.start_voice());
    assert_eq!(engine.voice_state(), VoiceState::Recording);
    feed(&mut engine, 1.0);
    assert_eq!(engine.voice_recorded_seconds(), 1.0);

    assert!(engine.stop_voice());
    assert_eq!(engine.voice_state(), VoiceState::Transcribing);
    assert_eq!(wait_for_text(&mut engine).unwrap(), "16000");
    assert_eq!(engine.voice_state(), VoiceState::Idle);
}

#[test]
fn a_too_short_recording_is_not_sent_to_the_recognizer() {
    let (mut engine, runs) = voice_engine();
    assert!(engine.start_voice());
    feed(&mut engine, MIN_VOICE_SECONDS / 2.0);

    assert!(!engine.stop_voice());
    assert_eq!(engine.voice_state(), VoiceState::Idle);
    assert_eq!(engine.poll_voice(), None);
    assert_eq!(runs.load(Ordering::SeqCst), 0, "太短的录音不该送去识别");
}

#[test]
fn samples_pushed_while_not_recording_are_dropped() {
    let (mut engine, _runs) = voice_engine();
    assert!(!engine.push_voice_samples(&[0.0; 100]));
}

#[test]
fn input_being_composed_blocks_starting_a_recording() {
    let (mut engine, _runs) = voice_engine();
    engine.push('k');
    assert!(!engine.start_voice());
    assert_eq!(engine.voice_state(), VoiceState::Idle);
}

#[test]
fn restarting_a_recording_discards_the_result_already_in_flight() {
    let (mut engine, runs) = voice_engine();
    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());

    // 旧结果还没取走就重开一段：回来时序号对不上，应当被丢掉
    assert!(engine.start_voice());
    wait_runs(&runs, 1);
    assert_eq!(engine.poll_voice(), None);
    assert_eq!(
        engine.voice_state(),
        VoiceState::Recording,
        "丢掉旧结果不该动新的一段"
    );
}

#[test]
fn cancelling_a_recording_discards_it_and_its_result() {
    let (mut engine, runs) = voice_engine();
    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());
    engine.cancel_voice();

    wait_runs(&runs, 1);
    assert_eq!(engine.voice_state(), VoiceState::Idle);
    assert_eq!(engine.poll_voice(), None);
}

#[test]
fn private_input_disables_and_cancels_voice() {
    let (mut engine, runs) = voice_engine();
    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());

    // 密码框：在飞的识别结果也不能落地
    engine.set_private(true);
    assert!(!engine.voice_available());
    assert!(!engine.start_voice());
    wait_runs(&runs, 1);
    assert_eq!(engine.poll_voice(), None);
    assert_eq!(engine.voice_state(), VoiceState::Idle);
}

#[test]
fn silence_settles_back_to_idle_without_text() {
    let mut engine = engine().with_speech_recognizer(Box::new(SilentRecognizer));
    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());

    assert_eq!(wait_for_text(&mut engine), None);
    assert_eq!(engine.voice_state(), VoiceState::Idle);
}

#[test]
fn recording_stops_at_the_limit_and_reports_truncation() {
    let (mut engine, _runs) = voice_engine();
    assert!(engine.start_voice());
    assert!(!engine.voice_truncated());

    feed(&mut engine, MAX_VOICE_SECONDS as f32 + 5.0);
    assert!(engine.voice_truncated());
    assert_eq!(engine.voice_recorded_seconds(), MAX_VOICE_SECONDS as f32);

    assert!(engine.stop_voice());
    let expected = (MAX_VOICE_SECONDS * VOICE_SAMPLE_RATE as usize).to_string();
    assert_eq!(wait_for_text(&mut engine).unwrap(), expected);
}

#[test]
fn a_voice_commit_is_logged_with_the_voice_source() {
    let entries = Arc::new(Mutex::new(Vec::new()));
    let runs = Arc::new(AtomicUsize::new(0));
    let mut engine = engine()
        .with_input_logger(Box::new(MemoryLogger(entries.clone())))
        .with_speech_recognizer(Box::new(FakeRecognizer { runs }));

    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());
    let text = wait_for_text(&mut engine).unwrap();
    assert_eq!(text, "16000");
    // 取到结果不等于上屏：文字要先给用户看过，接受（空格 / 数字）之后才记日志与个人 n-gram
    assert!(entries.lock().unwrap().is_empty(), "取结果不该记日志");
    assert_eq!(engine.accept_voice(&text), "16000");

    let entries = entries.lock().unwrap();
    assert_eq!(entries.len(), 1, "语音上屏只该记一条，不该顺带记 retype");
    let InputLogEntry::Commit(commit) = &entries[0] else {
        panic!("expected a commit");
    };
    assert_eq!(commit.source, InputSource::Voice);
    assert_eq!(commit.text, "16000");
    // 语音没有键、没有候选
    assert_eq!(commit.keys, "");
    assert_eq!(commit.index, None);
    assert!(commit.top.is_empty());
}

/// 丢弃就是什么都不做：Core 侧没有对应的调用，不上屏、不记日志、不进个人 n-gram。
/// 这是 `poll_voice` 与 `accept_voice` 拆开的原因 —— 原来「取到就上屏」在丢弃这条路上
/// 会留下一次用户没要过的学习。
#[test]
fn discarding_the_result_leaves_nothing_behind() {
    let entries = Arc::new(Mutex::new(Vec::new()));
    let runs = Arc::new(AtomicUsize::new(0));
    let mut engine = engine()
        .with_input_logger(Box::new(MemoryLogger(entries.clone())))
        .with_speech_recognizer(Box::new(FakeRecognizer { runs }));

    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());
    let text = wait_for_text(&mut engine).unwrap();

    assert_eq!(text, "16000");
    assert!(
        entries.lock().unwrap().is_empty(),
        "丢掉的结果不该进输入日志"
    );
}

#[test]
fn a_voice_commit_does_not_carry_the_previous_query() {
    let entries = Arc::new(Mutex::new(Vec::new()));
    let runs = Arc::new(AtomicUsize::new(0));
    let mut engine = engine()
        .with_input_logger(Box::new(MemoryLogger(entries.clone())))
        .with_speech_recognizer(Box::new(FakeRecognizer { runs }));

    // 先敲一段拼音查一次（不选），再清掉去语音上屏 —— 清掉的是缓冲区，不是上一次的查询摘要
    engine.set_input("kaifa");
    engine.query().unwrap();
    engine.clear();

    assert!(engine.start_voice());
    feed(&mut engine, 1.0);
    assert!(engine.stop_voice());
    let text = wait_for_text(&mut engine).unwrap();
    engine.accept_voice(&text);

    let entries = entries.lock().unwrap();
    let InputLogEntry::Commit(commit) = &entries[0] else {
        panic!("expected a commit");
    };
    assert_eq!(commit.scope, "", "上一次查询的作用域不该写进语音这条");
    assert!(commit.pinyin.is_empty(), "上一次查询的拼音不该写进语音这条");
    assert!(commit.top.is_empty(), "上一次查询的候选不该写进语音这条");
}
