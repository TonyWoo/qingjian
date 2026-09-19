//! 报光标位置：**没有组句**时也要让 Server 知道候选窗该摆在哪儿。
//!
//! 组句期间是 `composition::report_caret` 顺手报的（那会儿手里已经有编辑 cookie），
//! 语音候选恰恰落在没有组句的时候（说完话，拼音早没了），所以这里自己请一个只读会话去量。
//! 位置报得早一点没关系，Server 记着，等识别结果到了再摆窗口。

use windows::Win32::UI::TextServices::{
    ITfContext, ITfEditSession, ITfEditSession_Impl, TF_ES_READ,
};
use windows::core::{Result, implement};

use super::anchor::{anchor_rect, mouse_screen_rect, selection_range};
use crate::com::log::log;
use crate::com::service::SharedClient;

/// 一次性只读会话：量出光标（空选区就是光标）的屏幕矩形报给 Server。
#[implement(ITfEditSession)]
pub(crate) struct CaretSession {
    /// 目标文档上下文。
    context: ITfContext,

    /// 引擎层：把矩形发给 Server。
    engine: SharedClient,
}

impl ITfEditSession_Impl for CaretSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        // 量不到（有些应用对空选区给全零）就退到鼠标处，与组句那条路一致
        let rect = selection_range(&self.context, ec)
            .map(|range| anchor_rect(&self.context, ec, &range))
            .unwrap_or_else(mouse_screen_rect);
        if let Ok(mut guard) = self.engine.try_borrow_mut()
            && let Some(client) = guard.as_mut()
            && let Err(error) = client.position_candidates(rect)
        {
            log(&format!("报光标位置失败: {error}"));
        }
        Ok(())
    }
}

/// 请求一个异步只读会话量光标位置。`Ok` 只说明已受理。
pub(crate) fn request_caret(
    context: &ITfContext,
    client_id: u32,
    engine: SharedClient,
) -> Result<()> {
    let session = CaretSession {
        context: context.clone(),
        engine,
    };
    super::update::request(context, client_id, session.into(), TF_ES_READ)
}
