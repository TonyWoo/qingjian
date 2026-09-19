//! 按消息类型分派：会话开关、按键、轮询、失焦上屏、选区 / 光标矩形 / 中英模式的通知。

use qingjian_platform::protocol::{
    ClientMessage, Frame, KeyEvent, KeyOutcome, PROTOCOL_VERSION, ServerMessage, SessionId,
};

use super::Router;
use super::key::Effect;
use super::session::SessionInfo;

impl Router {
    pub(super) fn dispatch(&mut self, message: ClientMessage) -> Option<ServerMessage> {
        match message {
            ClientMessage::OpenSession {
                session,
                app,
                protocol,
            } => {
                tracing::debug!(?session, app, protocol, "会话打开");
                if protocol != PROTOCOL_VERSION {
                    tracing::warn!(
                        ?session,
                        app,
                        dll = protocol,
                        server = PROTOCOL_VERSION,
                        "DLL 与 Server 的协议版本不同（应用还没重启、用着旧 DLL？），照常服务"
                    );
                }
                // 同一会话重开（DLL 断线重连）：从干净状态起。
                if self.focused == Some(session) {
                    self.reset_composition();
                    self.focused = None;
                }
                self.sessions.insert(
                    session,
                    SessionInfo {
                        app,
                        private: false,
                    },
                );
                None
            }
            ClientMessage::Key { session, event } => Some(self.handle_key(session, event)),
            ClientMessage::Poll { session } => Some(self.handle_poll(session)),
            ClientMessage::Commit { session } => {
                let text = self.commit_raw_for(session);
                tracing::debug!(?session, ?text, "焦点离开，结束组句");
                Some(ServerMessage::Committed { session, text })
            }
            ClientMessage::Surrounding { session, text } => {
                tracing::trace!(?session, chars = text.chars().count(), "收到光标前文");
                self.set_surrounding(session, text);
                None
            }
            ClientMessage::Privacy { session, private } => {
                tracing::debug!(?session, private, "输入框私密状态");
                self.set_privacy(session, private);
                None
            }
            ClientMessage::Selection {
                session,
                request,
                text,
                rect,
            } => Some(self.handle_selection(session, request, text, rect)),
            ClientMessage::PositionCandidates { session, rect } => {
                self.position_candidates(session, rect);
                None
            }
            ClientMessage::HideCandidates { session } => {
                // 组句在 DLL 侧结束（应用终止组句 / 翻译评审失焦）：只收窗口；缓冲留给下一键的 Commit 清。
                if self.focused == Some(session) {
                    self.end_translation();
                    self.hide_candidate_window();
                }
                None
            }
            ClientMessage::ModeChanged { session, english } => {
                tracing::debug!(?session, english, "中英模式");
                self.handle_mode_changed(english);
                None
            }
            ClientMessage::SyncMode { session } => Some(ServerMessage::ModeSync {
                session,
                english: self.take_pending_mode(),
            }),
            ClientMessage::ImeSwitched { session } => {
                tracing::debug!(?session, "切成了别的输入法");
                self.handle_ime_switched();
                None
            }
            ClientMessage::CloseSession { session } => {
                self.sessions.remove(&session);
                if self.focused == Some(session) {
                    self.reset_composition();
                    self.focused = None;
                }
                self.flush_learning();
                tracing::debug!(?session, "会话关闭");
                None
            }
        }
    }

    fn handle_key(&mut self, session: SessionId, event: KeyEvent) -> ServerMessage {
        self.ensure_focus(session);
        self.notice = None;
        if self.translation.is_some() {
            return self.handle_translation_review(session, &event);
        }
        // 语音触发键：不组句时按一下开始录音、再按一下结束。与「翻译选中文字」一样在
        // apply_key 之前拦 —— 晚一步的话 Ctrl 系组合会被 has_command_key 放行给应用。
        // 起不来的话（没接模型、麦克风打不开、正在组句）不吃这个键，让它照常走。
        if self.engine.composition().is_empty()
            && self.matches_voice_combo(&event)
            && self.toggle_voice()
        {
            let frame = self.current_frame();
            return ServerMessage::KeyResult {
                session,
                outcome: KeyOutcome::Consumed,
                commit: None,
                frame,
            };
        }
        if self.engine.composition().is_empty()
            && self.engine.prediction_enabled()
            && self.matches_translate_combo(&event)
        {
            self.selection_seq += 1;
            self.pending_selection = Some(self.selection_seq);
            tracing::debug!(
                ?session,
                request = self.selection_seq,
                "翻译选中文字：请 DLL 读选区"
            );
            return ServerMessage::RequestSelection {
                session,
                request: self.selection_seq,
            };
        }
        let (mut commit, outcome) = match self.apply_key(&event) {
            Effect::Changed(commit) => {
                self.recompose();
                (commit, KeyOutcome::Consumed)
            }
            Effect::Navigated => (None, KeyOutcome::Consumed),
            Effect::Passthrough => (None, KeyOutcome::Passthrough),
        };
        // 语音识别结果挂起时，搭这次按键的车上屏。**只有被吃掉的键带得走**：
        // 放行且没有可打印字符的键，DLL 会把 commit 丢掉（key_sink.rs 的 `Next::Document`
        // 分支），带上等于丢字 —— 那种键就让它继续挂着等下一个。
        // 顺序上语音在前：话是先说的，键是后按的。
        if matches!(outcome, KeyOutcome::Consumed)
            && let Some(pending) = self.voice.take_pending()
        {
            tracing::info!(chars = pending.chars().count(), "语音文本随这次按键上屏");
            commit = Some(match commit {
                Some(text) => format!("{pending}{text}"),
                None => pending,
            });
        }
        self.poll_prediction();
        let frame = self.current_frame();
        self.reconcile_candidates(&frame);
        ServerMessage::KeyResult {
            session,
            outcome,
            commit,
            frame,
        }
    }

    /// 云联想轮询：聚焦会话拉一次异步结果回最新一帧，否则回空帧。释义兜底与本地整句模型也借这个节拍收。
    fn handle_poll(&mut self, session: SessionId) -> ServerMessage {
        self.tick();
        let learned = self.engine.poll_glosses();
        if learned > 0 {
            tracing::info!(learned, "释义兜底写入个人释义表");
        }
        let frame = if self.focused == Some(session) {
            self.poll_prediction();
            let frame = self.current_frame();
            self.reconcile_candidates(&frame);
            frame
        } else {
            Frame::default()
        };
        ServerMessage::Update { session, frame }
    }
}
