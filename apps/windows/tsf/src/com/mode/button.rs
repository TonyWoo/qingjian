//! 中 / 英输入模式指示器：Win11 托盘品牌图标左边的模式图标。按微软 IME 的做法经 `GUID_LBI_INPUTMODE`
//! 语言栏按钮把图标交给系统（转换模式 compartment 不走这条通道，光写它不显示）。Caps Lock 亮着显示「A」。

use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::{E_NOINTERFACE, POINT, RECT};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::UI::TextServices::{
    GUID_LBI_INPUTMODE, ITfLangBarItem_Impl, ITfLangBarItemButton, ITfLangBarItemButton_Impl,
    ITfLangBarItemSink, ITfMenu, ITfSource, ITfSource_Impl, TF_LANGBARITEMINFO,
    TF_LBI_STYLE_BTN_BUTTON, TfLBIClick,
};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, Ref, Result, implement};

use super::ModeState;
use super::icon::{self, Glyph};
use crate::com::CLSID_QINGJIAN;
use crate::com::key::event::caps_lock_on;
use crate::com::log::log;

/// 右键菜单里「设置…」那一项的 id，自己定、`OnMenuSelect` 按它分派；不用 0，语言栏拿 0 表示没选中。
const MENU_OPEN_SETTINGS: u32 = 1;

/// 与本 DLL 同目录的设置程序。安装包把 `qingjian_tsf.dll`、`qingjian-settings.exe` 都放进 `{app}`，
/// 开发时也同在 `target/release/`，所以从本 DLL 的路径推。
const SETTINGS_EXE: &str = "qingjian-settings.exe";

/// `GUID_LBI_INPUTMODE` 语言栏按钮：图标随 [`ModeState`] 显示中 / 英，点它切模式。
#[implement(ITfLangBarItemButton, ITfSource)]
pub(crate) struct ModeButton {
    state: Rc<ModeState>,
}

impl ModeButton {
    pub(crate) fn create(state: Rc<ModeState>) -> ITfLangBarItemButton {
        Self { state }.into()
    }
}

impl ITfLangBarItem_Impl for ModeButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        let info = unsafe { &mut *pinfo };
        info.clsidService = CLSID_QINGJIAN;
        info.guidItem = GUID_LBI_INPUTMODE;
        info.dwStyle = TF_LBI_STYLE_BTN_BUTTON;
        info.ulSort = 0;
        let desc: Vec<u16> = "青简中英模式".encode_utf16().collect();
        let n = desc.len().min(info.szDescription.len());
        info.szDescription[..n].copy_from_slice(&desc[..n]);
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _fshow: BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from("中 / 英（单击 Shift 切换）"))
    }
}

impl ITfLangBarItemButton_Impl for ModeButton_Impl {
    fn OnClick(&self, _click: TfLBIClick, _pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        crate::com::service::toggle_mode();
        Ok(())
    }

    /// 右键菜单。只一项「设置…」——系统给 IME 的「属性 / 选项」入口就落在这里，
    /// 与悬浮状态条上的 ⚙、开始菜单里的「青简设置」起的是同一个程序。
    fn InitMenu(&self, pmenu: Ref<ITfMenu>) -> Result<()> {
        let Ok(menu) = pmenu.ok() else {
            return Ok(());
        };
        // `cch` 不含结尾 NUL，但缓冲里留一个：weasel 与 PIME 都是这么备的
        // （weasel 用 HMENU 转过来、PIME 直接传 `c_str()`），万一语言栏按 C 字符串读也不越界。
        let mut text: Vec<u16> = "设置…".encode_utf16().collect();
        text.push(0);
        let item = &text[..text.len() - 1];
        unsafe {
            menu.AddMenuItem(
                MENU_OPEN_SETTINGS,
                0,
                HBITMAP::default(),
                HBITMAP::default(),
                item,
                std::ptr::null_mut(),
            )?;
        }
        Ok(())
    }

    fn OnMenuSelect(&self, wid: u32) -> Result<()> {
        if wid == MENU_OPEN_SETTINGS {
            open_settings();
        }
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        icon::make(self.glyph())
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(match self.glyph() {
            Glyph::Chinese => "中",
            Glyph::English => "英",
            Glyph::CapsLock => "A",
        }))
    }
}

impl ModeButton_Impl {
    /// Caps 亮着无论中英模式都直接出大写英文，所以它优先。
    fn glyph(&self) -> Glyph {
        if caps_lock_on() {
            Glyph::CapsLock
        } else if self.state.english() {
            Glyph::English
        } else {
            Glyph::Chinese
        }
    }
}

impl ITfSource_Impl for ModeButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<IUnknown>) -> Result<u32> {
        if unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(E_NOINTERFACE.into());
        }
        let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
        *self.state.sink.borrow_mut() = Some(sink);
        Ok(1) // 只支持一个回调，cookie 固定
    }

    fn UnadviseSink(&self, _dwcookie: u32) -> Result<()> {
        *self.state.sink.borrow_mut() = None;
        Ok(())
    }
}

/// 起与本 DLL 同目录的设置程序。失败只写日志：语言栏的菜单回调没有能报错的地方，
/// 而这里起不来通常意味着装的布局跟预期不一样，日志是唯一线索。
fn open_settings() {
    let exe = crate::com::module_path()
        .map(|dll| PathBuf::from(dll.to_string()).with_file_name(SETTINGS_EXE));
    match exe {
        Ok(exe) => {
            if let Err(error) = std::process::Command::new(&exe).spawn() {
                log(&format!("打开设置程序失败（{}）：{error}", exe.display()));
            }
        }
        Err(error) => log(&format!("拿不到本 DLL 的路径，打不开设置程序：{error}")),
    }
}
