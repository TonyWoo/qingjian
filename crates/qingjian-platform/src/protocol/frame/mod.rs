//! 一次要绘制的组句状态：preedit 行加候选页。

pub mod preedit;

pub use preedit::{PreeditKind, PreeditSegment};

use serde::{Deserialize, Serialize};

use qingjian_core::CandidateList;

use crate::{LayoutMode, ThemeMode};

/// 语音输入这一刻在等什么：DLL 靠它决定要不要拦空格 / 数字 / Esc，以及要不要接着轮询。
///
/// 语音的识别结果由 Server 攒着（它没有别的通道能主动往文档里写字），所以要有一段「等用户选」的
/// 时间。这段时间 DLL 必须知情：空格本来不经它转发（`would_eat` 放行给应用），不知情就拦不住。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoicePrompt {
    /// 录完了，正在识别：DLL 每拍拉一次 Server（结果一到就要能马上拦空格），不拦任何键。
    Transcribing,

    /// 认好了，等用户选：`text` 由 Server 画成候选窗里的一条，DLL 拦空格 / `1` 接受、`Esc` 丢弃。
    Ready {
        /// 认出来的文字（已是喂给应用的最终形态）。
        text: String,
    },
}

/// Server 告诉 DLL「现在屏幕上该是什么样」：组句的拼音行、候选页、高亮与页码。
/// 空 [`Frame`]（`preedit` 与 `candidates` 都空）表示没有在组句，DLL 收起候选窗口。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// 组句拼音行的分段，按顺序拼成整行。
    pub preedit: Vec<PreeditSegment>,

    /// 光标在拼音行里的位置，按 `preedit` 拼接后的字符（`char`）数算。
    pub cursor: usize,

    /// 当前页的候选（已排好序、不带译文由后续 [`super::ServerMessage::Update`] 补）。
    pub candidates: CandidateList,

    /// 当前页里高亮的候选下标（页内，从 0 起）。
    pub highlight: usize,

    /// 当前页码（从 0 起）。
    pub page: usize,

    /// 总页数；翻页键是否可用看它。
    pub page_count: usize,

    /// 候选排布（竖排 / 横排）。DLL 是纯渲染端，布局由 Server 按 `[general] layout` 配置随帧下发。
    pub layout: LayoutMode,

    /// 候选窗口外观（跟随系统 / 浅色 / 深色）。`System` 由 DLL 侧按当前系统主题解析。
    pub theme: ThemeMode,

    /// 整句补全（云联想给的整段拼音的整句结果）：画在 preedit 行右侧，按 Tab 上屏。无则 `None`。
    pub sentence: Option<String>,

    /// 屏幕提示（删候选后的「已删除…」一句）：画在 preedit 行下方，显示到下一次按键。无则 `None`。
    /// 不参与 [`is_empty`](Self::is_empty)：单有提示不算在组句，否则空组句也会撑开候选窗口。
    #[serde(default)]
    pub notice: Option<String>,

    /// 语音输入的状态（在认 / 等选）。无则 `None`。
    /// **不参与 [`is_empty`](Self::is_empty)** —— 语音不是组句：算进去的话 DLL 会当成在组句，
    /// 于是去轮询、去申请编辑会话、失焦时走 `commit_pending`，全是错的。
    #[serde(default)]
    pub voice: Option<VoicePrompt>,
}

impl Frame {
    /// 没有在组句：DLL 据此收起候选窗口。提示与语音都不算数（见 [`notice`](Self::notice)、[`voice`](Self::voice)）。
    pub fn is_empty(&self) -> bool {
        self.preedit.is_empty() && self.candidates.items.is_empty()
    }

    /// 语音认好了、等着用户选：要上屏的文字。
    pub fn voice_ready(&self) -> Option<&str> {
        match &self.voice {
            Some(VoicePrompt::Ready { text }) => Some(text),
            _ => None,
        }
    }

    /// 语音这边有状态（在认或在等选）：DLL 据此接着轮询。
    pub fn voice_pending(&self) -> bool {
        self.voice.is_some()
    }
}
