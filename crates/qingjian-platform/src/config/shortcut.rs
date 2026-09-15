use qingjian_core::ModeKeys;
use serde::{Deserialize, Serialize};

use super::key_combo::KeyCombo;
use super::modifiers::Modifiers;

/// 配置文件 `[shortcut]` 分节：前缀模式键（Core 的 [`ModeKeys`]）加壳层的修饰键组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    /// 表达式 / 问字模式键，键名与以前一样直接在分节下（`expression` / `question`）。
    #[serde(flatten)]
    pub mode: ModeKeys,

    /// 数字键配这些修饰键：上屏候选的第一个译词。
    pub translation: Modifiers,

    /// 数字键配这些修饰键：上屏候选的第二个译词（候选右侧有两个译词时）。
    pub translation_second: Modifiers,

    /// 把应用里选中的文字译成学习语言（需要云服务开着）。
    pub translate_selection: KeyCombo,

    /// 数字键配这些修饰键：删掉候选（用户词整个删掉，词库词清掉对它的学习）。
    pub delete_candidate: Modifiers,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        // Windows 上 Alt+数字被系统当菜单快捷键截走（TSF 收不到），译词键缺省用 Ctrl；macOS 用 Option。
        #[cfg(windows)]
        let (translation, translation_second) = (Modifiers::CONTROL, Modifiers::SHIFT_CONTROL);
        #[cfg(not(windows))]
        let (translation, translation_second) = (Modifiers::OPTION, Modifiers::SHIFT_OPTION);
        Self {
            mode: ModeKeys::default(),
            translation,
            translation_second,
            translate_selection: KeyCombo::TRANSLATE_DEFAULT,
            delete_candidate: Modifiers::SHIFT,
        }
    }
}

impl ShortcutConfig {
    /// 删候选的修饰键；为空或与任一组译词键撞了就退回缺省。
    pub fn delete_keys(&self) -> Modifiers {
        let (first, second) = self.translation_keys();
        if self.delete_candidate.is_empty()
            || self.delete_candidate == first
            || self.delete_candidate == second
        {
            Self::default().delete_candidate
        } else {
            self.delete_candidate
        }
    }

    /// 两组译词修饰键；两组相同或有一组为空时整个退回缺省，不做一半。
    pub fn translation_keys(&self) -> (Modifiers, Modifiers) {
        if self.translation == self.translation_second
            || self.translation.is_empty()
            || self.translation_second.is_empty()
        {
            let default = Self::default();
            (default.translation, default.translation_second)
        } else {
            (self.translation, self.translation_second)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_files_without_modifier_keys_still_parse_and_get_defaults() {
        // 译词键的缺省是分平台的（Windows 用 Ctrl，见 `Default`），断言的是「回到缺省」而不是某个平台的值
        let default = ShortcutConfig::default().translation_keys();
        let parsed: ShortcutConfig = toml::from_str("expression = \"i\"\n").unwrap();
        assert_eq!(parsed.mode.expression, 'i');
        assert_eq!(parsed.translation_keys(), default);
        // 两个键写成了同一个：非法组合，整个退回缺省
        let same: ShortcutConfig = toml::from_str(&format!(
            "translation = \"{}\"\ntranslation_second = \"{}\"\n",
            default.0.key(),
            default.0.key()
        ))
        .unwrap();
        assert_eq!(same.translation_keys(), default);
        // 两个键都给全且不同：原样用，不经缺省
        let swapped: ShortcutConfig =
            toml::from_str("translation = \"control+option\"\ntranslation_second = \"option\"\n")
                .unwrap();
        assert_eq!(swapped.translation_keys().1, Modifiers::OPTION);
    }

    #[test]
    fn delete_keys_fall_back_when_clashing_with_translation_keys() {
        let parsed: ShortcutConfig = toml::from_str("").unwrap();
        assert_eq!(parsed.delete_keys(), Modifiers::SHIFT);
        // 撞上译词键（缺省值分平台，所以从缺省里取）时删候选键退回 Shift
        let clash: ShortcutConfig = toml::from_str(&format!(
            "delete_candidate = \"{}\"\n",
            ShortcutConfig::default().translation.key()
        ))
        .unwrap();
        assert_eq!(clash.delete_keys(), Modifiers::SHIFT);
        // 不撞译词键时按配置用。取 command：两组缺省（option / shift+option / control / shift+control）
        // 在任何平台上都不含它，而 control+shift 在 Windows 上正好是缺省的第二组
        let custom: ShortcutConfig = toml::from_str("delete_candidate = \"command\"\n").unwrap();
        assert!(custom.delete_keys().command);
    }
}
