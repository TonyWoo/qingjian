//! 「语音」页：本地离线语音输入的开关、模型下载与已装状态。
//!
//! 下载在后台线程跑（照云服务「测试连接」那套 `spawn_background`）。**这里没有实时百分比**：
//! 组件只有收到消息才会重绘，而后台闭包要求 `Send`、拿不到 `LocalSender`（它内部是 `Rc`），
//! 所以中途报不了进度。用不定式的进度条 + 一句体积说明代替，下完再出结果。

use std::path::PathBuf;

use qingjian_voice::fetch::{Download, FetchEvent, Tier};
use windows_reactor::*;

use crate::panel::controls::{field, labeled, note, page};
use crate::panel::voice_status::VoiceStatus;
use crate::panel::{Message, Settings};

/// 后台下一档模型，轮询到出结果或被取消。
pub(crate) fn run_download(
    tier: Tier,
    dir: PathBuf,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let download = Download::spawn(&tier, dir);
    loop {
        if cancel.is_cancelled() {
            download.cancel();
            return Err("已取消".to_owned());
        }
        match download.poll() {
            Some(FetchEvent::Done { dir }) => {
                return Ok(format!("已下好，放在 {}", dir.display()));
            }
            Some(FetchEvent::Failed(error)) => return Err(error.to_string()),
            // 进度收下但不显示，见模块头
            Some(FetchEvent::Progress { .. }) => {}
            None if download.is_finished() => return Err("下载线程意外结束了".to_owned()),
            None => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

pub(crate) fn view(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let voice = &settings.config.voice;
    let downloading = matches!(settings.voice_status, VoiceStatus::Downloading(_));
    let status = match &settings.voice_status {
        VoiceStatus::Idle => String::new(),
        VoiceStatus::Downloading(name) => {
            format!("正在下载 {name}…可以关掉这个窗口，但别关机。")
        }
        VoiceStatus::Ok(message) => message.clone(),
        VoiceStatus::Failed(message) => format!("失败：{message}"),
    };

    let mut rows: Vec<KeyedView> = Vec::new();
    rows.push(KeyedView::new(
        "enable",
        field(
            "启用语音输入",
            "按 [shortcut] voice 指定的键（缺省 Ctrl+Shift+V）开始录音、再按一下结束，识别结果在下一次按键时上屏。全程离线，音频不出本机；密码框等私密输入里不会录音。",
            ToggleSwitch::new()
                .is_on(voice.enabled)
                .on_toggled(context.callback(Message::VoiceEnabled)),
        ),
    ));
    rows.push(KeyedView::new(
        "installed",
        field(
            "已装模型",
            "模型不随安装包发布（几百 MB，多数人不用语音），要在这里按需下载；下好之后自动打开开关。",
            TextBlock::new()
                .text_wrapping(TextWrapping::Wrap)
                .text(match &settings.voice_model {
                    Some(dir) => dir.display().to_string(),
                    None => "还没有可用的模型".to_owned(),
                }),
        ),
    ));

    // 档位清单：一行一档、各自一个下载按钮。清单来自编进二进制的 voice.lock，
    // 资产还没发布时它是空的。
    let tiers: Vec<(&'static str, &'static Tier)> = qingjian_voice::fetch::tiers().collect();
    if tiers.is_empty() {
        rows.push(KeyedView::new(
            "empty",
            note(
                "这个版本还没有可下载的模型（发布清单是空的）。手动放一个模型目录到数据目录的 voice\\ 下也能直接用。",
            ),
        ));
    }
    for (name, tier) in tiers {
        let hint = format!("{}（{}）", tier.note, tier.size_text());
        rows.push(KeyedView::new(
            name,
            field(
                &tier.label,
                &hint,
                Button::new()
                    .is_enabled(!downloading)
                    .on_click(context.message(Message::VoiceDownload(name.to_owned())))
                    .content("下载"),
            ),
        ));
    }

    if downloading {
        rows.push(KeyedView::new(
            "progress",
            labeled(
                "",
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .children((
                        ProgressBar::new().is_indeterminate(true).width(160.0),
                        TextBlock::new().text(status),
                    )),
            ),
        ));
    } else if !status.is_empty() {
        rows.push(KeyedView::new("status", note(&status)));
    }

    page("语音", StackPanel::new().spacing(16.0).keyed_children(rows))
}
