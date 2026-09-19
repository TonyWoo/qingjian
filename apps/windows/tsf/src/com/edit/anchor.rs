//! 候选窗口的定位锚点：组句范围 / 选区在屏幕上的矩形，拿不到时退到鼠标位置。

use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::TextServices::{ITfContext, ITfRange, TF_DEFAULT_SELECTION, TF_SELECTION};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::BOOL;

use qingjian_platform::protocol::ScreenRect;

/// 当前选区（光标就是空选区）。读选区与量光标位置都要，所以放在这里共用。
pub(crate) fn selection_range(context: &ITfContext, ec: u32) -> Option<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe {
        context
            .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
            .ok()?;
    }
    if fetched == 0 {
        return None;
    }
    // GetSelection 移交 range 的所有权（ManuallyDrop），取出后由调用方释放。
    unsafe { ManuallyDrop::take(&mut selection[0].range) }
}

/// `range` 的屏幕矩形，拿不到（有些应用给全零 / 空矩形）退到鼠标处。
pub(crate) fn anchor_rect(context: &ITfContext, ec: u32, range: &ITfRange) -> ScreenRect {
    to_screen(range_rect(context, ec, range).unwrap_or_else(mouse_anchor))
}

/// 没有可量的范围时的锚点：鼠标处一个零宽、约一行高的矩形。
pub(crate) fn mouse_screen_rect() -> ScreenRect {
    to_screen(mouse_anchor())
}

fn range_rect(context: &ITfContext, ec: u32, range: &ITfRange) -> Option<RECT> {
    let mut rect = RECT::default();
    let mut clipped = BOOL(0);
    unsafe {
        let view = context.GetActiveView().ok()?;
        view.GetTextExt(ec, range, &mut rect, &mut clipped).ok()?;
    }
    (rect.right > rect.left || rect.bottom > rect.top).then_some(rect)
}

fn mouse_anchor() -> RECT {
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    RECT {
        left: point.x,
        top: point.y,
        right: point.x,
        bottom: point.y + 16,
    }
}

fn to_screen(rect: RECT) -> ScreenRect {
    ScreenRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}
