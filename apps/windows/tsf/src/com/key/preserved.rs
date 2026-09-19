//! 带修饰键的快捷键登记成 TSF **保留键**（preserved key）。这类组合是系统键，不经击键 sink
//! （真机：Ctrl+Alt+T 与 Ctrl+Shift+V 在 `OnTestKeyDown` 里从没出现过 —— 日志里一整天的
//! `ctrl=true` 记录是 0 条）；保留键由 TSF 在应用之前匹配、回调 `OnPreservedKey`，UWP 里也一样。
//!
//! 现在登记两个：`[shortcut] translate_selection`（翻译选中文字）与 `[shortcut] voice`（语音输入）。
//! 激活时各读一次配置（AppContainer 读不到用户目录时用缺省）；**改了配置要切走再切回输入法才重新登记**。
//!
//! 命中时都走 [`key_event`] 合成一个带物理修饰键的按键喂给 Server，Server 侧再按
//! `matches_translate_combo` / `matches_voice_combo` 比一次 —— 所以 Server 那边不需要知道保留键这回事。

use windows::Win32::UI::TextServices::{
    ITfKeystrokeMgr, TF_MOD_ALT, TF_MOD_CONTROL, TF_MOD_SHIFT, TF_PRESERVEDKEY,
};
use windows::core::{GUID, Result};

use qingjian_platform::protocol::{KeyEvent, KeyModifiers};
use qingjian_platform::{Config, KeyCombo};

use crate::com::log::log;

/// 「翻译选中文字」保留键的标识，`OnPreservedKey` 按它认。
pub(crate) const GUID_TRANSLATE: GUID = GUID::from_u128(0x5c0a7b12_3d4e_4f60_8a91_2b3c4d5e6f70);

/// 「语音输入」保留键的标识。与上一条只差最后一位，但必须是不同的 GUID。
pub(crate) const GUID_VOICE: GUID = GUID::from_u128(0x5c0a7b12_3d4e_4f60_8a91_2b3c4d5e6f71);

/// msctf.h 的 `TF_MOD_LWIN`（windows crate 没导出）。
const TF_MOD_LWIN: u32 = 0x08;

/// 读 `%APPDATA%\Qingjian\config.toml` 里的翻译快捷键；读不到 / 解析失败用缺省。
pub(crate) fn load_combo() -> KeyCombo {
    let Some(path) = qingjian_platform::dirs::config_path() else {
        return KeyCombo::TRANSLATE_DEFAULT;
    };
    match Config::load(&path) {
        Ok(config) => config.shortcut.translate_selection,
        Err(error) => {
            log(&format!("读配置取翻译快捷键失败，用缺省: {error}"));
            KeyCombo::TRANSLATE_DEFAULT
        }
    }
}

/// 读 `[shortcut] voice`；读不到 / 解析失败用缺省。走 `voice_combo()` 而不是直接读字段 ——
/// 与翻译键撞了时那边会退回缺省，两边得用同一套判断，否则会登记同一个组合两次。
pub(crate) fn load_voice_combo() -> KeyCombo {
    let Some(path) = qingjian_platform::dirs::config_path() else {
        return KeyCombo::VOICE_DEFAULT;
    };
    match Config::load(&path) {
        Ok(config) => config.shortcut.voice_combo(),
        Err(error) => {
            log(&format!("读配置取语音快捷键失败，用缺省: {error}"));
            KeyCombo::VOICE_DEFAULT
        }
    }
}

fn preserved_key(combo: KeyCombo) -> TF_PRESERVEDKEY {
    let m = combo.modifiers;
    let mut modifiers = 0;
    if m.control {
        modifiers |= TF_MOD_CONTROL;
    }
    if m.option {
        modifiers |= TF_MOD_ALT;
    }
    if m.shift {
        modifiers |= TF_MOD_SHIFT;
    }
    if m.command {
        modifiers |= TF_MOD_LWIN;
    }
    TF_PRESERVEDKEY {
        uVKey: combo.key.to_ascii_uppercase() as u32,
        uModifiers: modifiers,
    }
}

pub(crate) fn register(
    keystroke: &ITfKeystrokeMgr,
    tid: u32,
    guid: &GUID,
    description: &str,
    combo: KeyCombo,
) -> Result<()> {
    let key = preserved_key(combo);
    let description: Vec<u16> = description.encode_utf16().collect();
    unsafe { keystroke.PreserveKey(tid, guid, &key, &description) }
}

pub(crate) fn unregister(keystroke: &ITfKeystrokeMgr, guid: &GUID, combo: KeyCombo) {
    let key = preserved_key(combo);
    let _ = unsafe { keystroke.UnpreserveKey(guid, &key) };
}

/// 保留键命中时喂给 Server 的按键：Router 按字符 + 物理修饰键与配置比对。
pub(crate) fn key_event(combo: KeyCombo, english_mode: bool) -> KeyEvent {
    let m = combo.modifiers;
    KeyEvent::new(
        combo.key.to_ascii_uppercase() as u32,
        Some(combo.key),
        KeyModifiers {
            ctrl: m.control,
            shift: m.shift,
            alt: m.option,
            win: m.command,
            caps: false,
            english_mode,
        },
    )
}
