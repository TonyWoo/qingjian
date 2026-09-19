use qingjian_platform::ThemeMode;

/// 状态条上语音那一格显示什么。
///
/// 语音是唯一「按了之后什么都不会发生」的输入方式：开始录音不上屏、停下之后还要等识别，
/// 中间这几秒用户看不见任何东西，就会以为按键没生效（真机踩过）。Cue 让状态条把这三个
/// 阶段说出来。中英模式走 DLL 推，语音状态不走 —— DLL 只管按键，状态条是 Server 自己画的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceCue {
    /// 没在录、也没有结果挂着：这一格不出现。
    #[default]
    Off,

    /// 正在录音：再按一下触发键结束。
    Recording,

    /// 录完了，正在识别。
    Transcribing,

    /// 认好了，文字摆在候选窗里等用户选（空格 / `1` 接受、`Esc` 丢弃）。
    Ready,
}

/// 状态条一次要显示的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    /// 语音那一格；`Off` 时整格不出现。
    pub voice: VoiceCue,

    /// 英文模式（`false` 中文）。
    pub english: bool,

    /// 是否啟用大千注音。
    pub zhuyin: bool,

    /// 开着双拼时的方案名，中文格里跟在「中」后面。
    pub scheme: Option<String>,

    /// 当前模式的全角标点开着（中英各记一份配置）；关着时格子显示 `,.` 画成灰的。
    pub full_width: bool,

    /// 外观模式。
    pub theme: ThemeMode,

    /// 配置里记住的内容左上角物理像素；`None` 首次按屏幕右下角摆。
    pub anchor: Option<(i32, i32)>,
}
