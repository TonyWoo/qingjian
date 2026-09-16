//! 输入方案：全拼 / 双拼四套 / 大千注音 / 形码（五笔）。配置项 `[general] scheme` 的值。
//!
//! 「输入方案是配置项，不是模式」：中英切换始终是布尔，换方案不改变别的方案的既定按键行为。
//! 这里只描述「用哪套方案」，怎么装配到引擎由各壳自己做（形码还要先按平台找出码表文件）。

use std::fmt;
use std::str::FromStr;

use qingjian_core::ShuangpinScheme;

/// 输入方案。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scheme {
    /// 全拼。
    #[default]
    Pinyin,

    /// 双拼，四套键位见 [`ShuangpinScheme`]。
    Shuangpin(ShuangpinScheme),

    /// 大千注音。
    Zhuyin,

    /// 86 五笔（形码）。
    Wubi86,
}

impl Scheme {
    /// 全部方案，设置界面与状态条按这个顺序列。
    pub const ALL: [Self; 7] = [
        Self::Pinyin,
        Self::Shuangpin(ShuangpinScheme::Xiaohe),
        Self::Shuangpin(ShuangpinScheme::Ziranma),
        Self::Shuangpin(ShuangpinScheme::Microsoft),
        Self::Shuangpin(ShuangpinScheme::Sogou),
        Self::Zhuyin,
        Self::Wubi86,
    ];

    /// 配置文件里的写法。`const`：设置界面按 [`Self::ALL`] 直接建常量表，不再手抄一份。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Pinyin => "pinyin",
            Self::Shuangpin(scheme) => scheme.key(),
            Self::Zhuyin => "zhuyin",
            Self::Wubi86 => "wubi86",
        }
    }

    /// 界面上的名字。`const` 的理由同上。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pinyin => "全拼",
            Self::Shuangpin(scheme) => scheme.label(),
            Self::Zhuyin => "大千注音",
            Self::Wubi86 => "五笔（86）",
        }
    }

    /// 是不是形码：候选不走拼音那一套（切分、简拼、模糊音、纠错、整句都不用），
    /// 引擎那边要挂码表，见 `qingjian_core::Engine::set_code_table`。
    pub const fn is_code(self) -> bool {
        matches!(self, Self::Wubi86)
    }

    /// 这套方案是双拼时是哪一套；装配引擎用（不是双拼时为 `None`）。
    pub const fn shuangpin(self) -> Option<ShuangpinScheme> {
        match self {
            Self::Shuangpin(scheme) => Some(scheme),
            _ => None,
        }
    }
}

impl FromStr for Scheme {
    type Err = String;

    /// 认不出来的写法报错，由调用方决定退回什么（配置层退回全拼并警告）。
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let key = text.trim();
        match key {
            "" | "pinyin" => Ok(Self::Pinyin),
            "zhuyin" => Ok(Self::Zhuyin),
            // `wubi` 是早期手工配置里可能写过的简写
            "wubi" | "wubi86" => Ok(Self::Wubi86),
            other => other.parse().map(Self::Shuangpin),
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_key_it_prints() {
        for scheme in Scheme::ALL {
            assert_eq!(scheme.key().parse::<Scheme>(), Ok(scheme), "{scheme}");
        }
    }

    #[test]
    fn empty_and_unknown_spellings() {
        assert_eq!("".parse::<Scheme>(), Ok(Scheme::Pinyin));
        assert_eq!(" pinyin ".parse::<Scheme>(), Ok(Scheme::Pinyin));
        assert_eq!("wubi".parse::<Scheme>(), Ok(Scheme::Wubi86));
        assert!("flypy".parse::<Scheme>().is_err());
    }

    #[test]
    fn knows_which_schemes_are_code_tables() {
        assert!(Scheme::Wubi86.is_code());
        assert!(!Scheme::Zhuyin.is_code());
        assert!(!Scheme::Shuangpin(ShuangpinScheme::Xiaohe).is_code());
        assert!(!Scheme::Pinyin.is_code());
    }
}
