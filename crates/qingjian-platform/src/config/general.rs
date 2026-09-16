use qingjian_core::ShuangpinScheme;
use serde::{Deserialize, Serialize};

use super::scheme::Scheme;
use super::{LayoutMode, LogLevel, PreeditMode, ThemeMode};

/// 每页最多几个候选：数字键只有 1–9。
pub const MAX_PAGE_SIZE: usize = 9;

/// 翻页键对的可选值，第一项是缺省：第一个键向前、第二个向后。`-` `=` 不在其中，`-` 已经是英文直输段的入口。
/// 缺省不用 `,` `.`：组句中敲逗号句号应该把首选上屏再补一个全角标点（`nihao,zaima` 一气打完），
/// 拿它们翻页就得先按空格再敲标点。
pub const PAGE_KEY_OPTIONS: [&str; 2] = ["[]", ",."];

/// 缺省翻页键对，与 [`PAGE_KEY_OPTIONS`] 第一项一致。
pub const DEFAULT_PAGE_KEYS: (char, char) = ('[', ']');

/// `[general]` 分节：与具体功能无关的常规项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// 学习语言（ISO 639-1，`en` / `ja` / `es`）：候选旁显示哪种语言的译文。要有对应的释义表文件才生效。
    pub learning_language: String,

    /// 每页候选数，1–9。
    pub page_size: usize,

    /// 翻页键对，两个字符：前一个上一页、后一个下一页。
    pub page_keys: String,

    /// 候选窗口外观。
    pub theme: ThemeMode,

    /// 候选窗口竖排 / 横排。
    pub layout: LayoutMode,

    /// 组句中的拼音显示在行内、候选窗口还是两处都显示。
    pub preedit: PreeditMode,

    /// 英文模式（Caps Lock 亮着）是否给英文候选（补全与拼错纠正）。关掉就是纯直通。
    pub english_candidates: bool,

    /// 中文模式下不在组句时敲的标点转成全角（`，。？！` 等，数字后的 `.` 保持半角）。
    /// Windows 悬浮状态条上可点切换；macOS 在偏好设置中选择默认模式。
    pub full_width_punctuation: bool,

    /// 英文模式下的同一件事，中英各记一份；缺省半角。只有 Windows 用（macOS 英文模式一律半角）。
    pub english_full_width_punctuation: bool,

    /// 输入方案：`pinyin`（全拼，缺省）/ `xiaohe` / `ziranma` / `microsoft` / `sogou` / `zhuyin` / `wubi86`，
    /// 见 [`Scheme`]。用不认识的写法时按全拼并警告。
    pub scheme: String,

    /// 旧键（2026-09-16 之前是 `[general] shuangpin`，空串为全拼）：只在 [`Self::scheme`] 里用来推断方案，
    /// 不再写出去；`scheme` 写了值就不看它。当时 `shuangpin` 与 `zhuyin` 是两个字段表达同一个维度。
    pub shuangpin: Option<String>,

    /// 旧键（同上的 `[general] zhuyin`）：同上。
    pub zhuyin: Option<bool>,

    /// 日志级别，缺省 info（不含用户敲的内容）。
    pub log_level: LogLevel,

    /// 输入日志：每次上屏记一行到数据目录的 `input-log.jsonl`（敲的键、看到的候选、选了什么），只写本机，
    /// 给离线回归评测与个人模型用。缺省开；关掉就不记，「高级」页可清空。
    pub input_log: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            learning_language: "en".to_owned(),
            page_size: MAX_PAGE_SIZE,
            page_keys: PAGE_KEY_OPTIONS[0].to_owned(),
            theme: ThemeMode::default(),
            layout: LayoutMode::default(),
            preedit: PreeditMode::default(),
            english_candidates: true,
            full_width_punctuation: true,
            english_full_width_punctuation: false,
            scheme: Scheme::default().key().to_owned(),
            shuangpin: None,
            zhuyin: None,
            log_level: LogLevel::default(),
            input_log: true,
        }
    }
}

impl GeneralConfig {
    /// 输入方案。`scheme` 没写时用旧键（`shuangpin` / `zhuyin`）推，都没有就是全拼。
    pub fn scheme(&self) -> Scheme {
        let key = self.scheme.trim();
        if !key.is_empty() {
            return match key.parse() {
                Ok(scheme) => scheme,
                Err(_) => {
                    tracing::warn!(key, "不认识的输入方案，按全拼");
                    Scheme::Pinyin
                }
            };
        }
        if self.zhuyin == Some(true) {
            tracing::info!("[general] zhuyin 已并入 scheme，可改成 scheme = \"zhuyin\"");
            return Scheme::Zhuyin;
        }
        let Some(legacy) = self
            .shuangpin
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Scheme::Pinyin;
        };
        match legacy.parse::<ShuangpinScheme>() {
            Ok(scheme) => {
                tracing::info!(
                    key = scheme.key(),
                    "[general] shuangpin 已并入 scheme，可改成它"
                );
                Scheme::Shuangpin(scheme)
            }
            Err(_) => {
                tracing::warn!(key = legacy, "不认识的双拼方案，按全拼");
                Scheme::Pinyin
            }
        }
    }

    /// 当前方案是双拼时是哪一套；不是双拼时为 `None`。
    pub fn shuangpin(&self) -> Option<ShuangpinScheme> {
        self.scheme().shuangpin()
    }

    /// 当前方案是不是大千注音。
    pub fn is_zhuyin(&self) -> bool {
        self.scheme() == Scheme::Zhuyin
    }

    /// 夹到合法范围的每页候选数。
    pub fn page_size(&self) -> usize {
        self.page_size.clamp(1, MAX_PAGE_SIZE)
    }

    /// 翻页键对；写得不对（不是两个不同的 ASCII 可见字符）时退回缺省。
    pub fn page_keys(&self) -> (char, char) {
        let mut chars = self.page_keys.chars();
        match (chars.next(), chars.next(), chars.next()) {
            (Some(previous), Some(next), None)
                if previous != next
                    && previous.is_ascii_graphic()
                    && next.is_ascii_graphic()
                    && !previous.is_ascii_alphanumeric()
                    && !next.is_ascii_alphanumeric() =>
            {
                (previous, next)
            }
            _ => DEFAULT_PAGE_KEYS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_and_keys_are_sanitized() {
        let mut general = GeneralConfig::default();
        assert_eq!(general.page_size(), 9);
        assert_eq!(general.page_keys(), ('[', ']'));
        general.page_size = 0;
        general.page_keys = ",.".to_owned();
        assert_eq!(general.page_size(), 1);
        assert_eq!(general.page_keys(), (',', '.'));
        general.page_size = 42;
        general.page_keys = "ab".to_owned();
        assert_eq!(general.page_size(), 9);
        assert_eq!(general.page_keys(), ('[', ']'));
        general.page_keys = ",,".to_owned();
        assert_eq!(general.page_keys(), ('[', ']'));
    }

    #[test]
    fn scheme_defaults_to_pinyin_and_unknown_names_fall_back() {
        let mut general = GeneralConfig::default();
        assert_eq!(general.scheme(), Scheme::Pinyin);
        general.scheme = "xiaohe".to_owned();
        assert_eq!(general.shuangpin(), Some(ShuangpinScheme::Xiaohe));
        general.scheme = " Sogou ".to_owned();
        assert_eq!(general.shuangpin(), Some(ShuangpinScheme::Sogou));
        general.scheme = "wubi86".to_owned();
        assert_eq!(general.scheme(), Scheme::Wubi86);
        assert!(general.scheme().is_code());
        general.scheme = "flypy".to_owned();
        assert_eq!(general.scheme(), Scheme::Pinyin);
    }

    #[test]
    fn legacy_shuangpin_and_zhuyin_keys_are_read_but_scheme_wins() {
        let mut general = GeneralConfig {
            scheme: String::new(),
            ..GeneralConfig::default()
        };
        assert_eq!(general.scheme(), Scheme::Pinyin);
        general.shuangpin = Some("xiaohe".to_owned());
        assert_eq!(general.scheme(), Scheme::Shuangpin(ShuangpinScheme::Xiaohe));
        general.zhuyin = Some(true);
        assert_eq!(general.scheme(), Scheme::Zhuyin);
        assert!(general.is_zhuyin());
        // 旧配置里两个都写是不合法的，新键写了就不看它们
        general.scheme = "pinyin".to_owned();
        assert_eq!(general.scheme(), Scheme::Pinyin);
        assert!(!general.is_zhuyin());
    }
}
