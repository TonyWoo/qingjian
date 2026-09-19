use serde::{Deserialize, Serialize};

/// 配置文件 `[voice]` 分节：本地离线语音输入。
///
/// 识别模型**不随包**（几百 MB，多数用户不用语音），要用户在设置里按需下载。
/// 模型落在用户数据目录的 `voice/<档位>/` 下；那里没有模型时这个开关无效，
/// 输入法照常用键盘，不会有任何异常。
///
/// 触发键不在这里 —— 按仓库约定快捷键一律进 `[shortcut]`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// 开着才接语音识别器、响应语音触发键。
    ///
    /// 缺省关：模型要用户先下，而且语音是最私密的一类输入，不该默认就开着监听。
    pub enabled: bool,

    /// 用哪一档模型。空串表示清单里的第一档（档位清单在 `qingjian-voice` 里，
    /// 这个 crate 被 TSF DLL 依赖，不能反过来依赖它，所以这里只存名字）。
    pub tier: String,
}
