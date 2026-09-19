//! 状态条一格的动作。

/// 状态条上一格点下去做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusAction {
    /// 「中 / 英」：切模式。
    ToggleMode,

    /// 「，。/ ,.」：切全角标点。
    TogglePunctuation,

    /// 齿轮：打开设置程序（UI 线程直接起进程，不经 Router）。
    OpenSettings,

    /// 语音状态格：只显示，不响应点击。
    ///
    /// 点击是「x 落在哪个右边界之前」来分格的，它出现时在最左边，所以必须占一格 ——
    /// 不在表里的话，点它会被算到右边那格（中 / 英）头上。
    None,
}
