# 本地语音输入（2026-09-18）

## 起因

想让青简能**离线**语音输入：本机麦克风收音、本地模型识别成文字、直接上屏，全程不联网、不上传音频。
与云联想不同，这一条的卖点就是「不联网」——音频是最私密的一类输入，不该出本机。

## 产品决策

| 项 | 决定 | 理由 |
|---|---|---|
| 触发 | 按一下开始录音、再按一下结束（toggle） | 长段落听写不用一直按着；唤醒词常驻监听与「输入优先、不许添延迟」冲突 |
| 上屏 | 识别结果**当作候选摆进候选窗**，空格 / `1` 接受、`Esc` 丢弃 | 见下 |
| 反馈 | 悬浮状态条最左边加一格：**录音中 / 识别中 / 空格上屏** | 语音是唯一「按下去当场什么都不发生」的输入方式（开始不落字、停下还要等识别）。真机第一次测就是按了没反应、以为按键没生效 —— 没有这一格，这个功能在用户眼里就是坏的 |
| 定位 | **短句输入** | Whisper 在 CPU 上 RTF 约 0.25，说 10 秒要等约 2.5 秒。不做 VAD 分段边说边上屏，保住「整段一次出结果」这个简单形态 |
| 学习 | 只记**个人 n-gram**，不进词频 / 用户词 | 见下 |
| 日志 | 进输入日志，`source` 新增 `Voice` | 见下 |

**为什么改成「进候选窗选一下」（2026-09-19 改，原先是「直接上屏」）**：直接上屏在实现上只有一条路 ——
Server 没有通道能主动往文档里写字，写字只有 DLL 在按键路径上做得到。于是「说完话文字自己出来」要么给 poll
加一条上屏通道、要么让 DLL 在非按键时机申请编辑会话插字，两处风险都只能压到真机上试。更要紧的是体验：
用户看不见识别成了什么，也就没有反悔的余地，说错一句只能删掉重来。改成候选窗里的一条候选之后，
上屏走的还是既有那条路（`KeyResult.commit`），识别结果看得见、选得动、丢得掉。
代价是 DLL 得知道「语音候选正挂着」才拦得住空格（空格平时不经 DLL 转发），所以帧上加了 `VoicePrompt`
（协议 +1），DLL 每拍拉一次 Server 同步它。

**学习与日志跟着「接受」走**：`poll_voice` 只取结果、不上屏，`accept_voice` 才记个人 n-gram 与输入日志 ——
丢弃那条路什么都不留（原来「取到就上屏」会让用户没要过的文本进学习，且撤不回来）。

**为什么学习只记 n-gram**：`Learner` 的 `record` / `record_choice` / `learn_word` **全部以拼音音节为键**，
语音没有音节。硬塞要反查读音，而多音字会造出错读音写进 `learn_word`，反过来污染拼音输入且**没法撤销**
（`apply_retraction` 要求同一段拼音）。既有代码已经做过同样的判断：`accept_prediction`（云端整句）
面对的处境一模一样 —— 一段没有拼音的整句文本 —— 它也只记个人 n-gram。跟着走，两条路上用户看到的学习效果才一致。

**为什么日志要记**：担心的「污染回放评测」现有代码已经处理 —— `--replay` 的 `tally_for` 对 `Cloud` /
`CloudSentence` / `Raw` / `Custom` 本来就返回 `None`（只计数、不评命中率），且它与 `meter_commit` 都是
**穷尽 match、没有 `_` 兜底**，加一个变体会强制编译器把两个决定点摆到面前。语音只进「不评的来源」那一行。
`INPUT_LOG_VERSION` 因此从 1 升到 2：新取值不是新字段，旧读端遇到不认识的枚举值会整行解析失败，不在
`plan/model-eval.md` 那条「加字段不升版本」的豁免里。

## 边界：哪边归 Core，哪边归平台层

- **音频采集 = 平台层的活**。麦克风是和键盘同级的系统输入设备，「把系统输入事件翻译成 Core 的输入」正是平台层职责。
- **语音识别 = Core 的活**。它是文本变换，平台层不允许出现文本变换。判断标准不变：把 IMK 换成 TSF，Core 一行不用改。
- **`crates/qingjian-voice`** 实现 Core 定义的 trait，方向是它依赖 Core（trait 反转），与 `qingjian-neural` 同构。

采样格式统一到 16 kHz 单声道 f32，**归一化放在采集侧**：这是 sherpa-onnx 自己的契约，而采样率与声道数是平台属性 ——
Core 一旦知道 44.1 kHz 这回事，换平台就可能要动 Core。Core 的依赖表保持 6 个。

## 关键区分：在线模型 vs 离线模型

这一条是调研里最容易踩的坑。sherpa-onnx 有两套识别器，模型不通用：

| | 识别器 | 接口 | 代表模型 |
|---|---|---|---|
| **离线** | `OfflineRecognizer` | 整段进、整段出 | SenseVoice、Whisper、Paraformer、offline transducer |
| **在线（流式）** | `OnlineRecognizer` | 会话式，逐块喂 | `streaming-zipformer-*` |

产品上选了「整段出结果」，对应的就是 `OfflineRecognizer`，所以 Core 的 trait 也是整段式的：

```rust
pub trait SpeechRecognizer: Send {
    fn transcribe(&mut self, samples: &[f32]) -> Option<String>;
}
```

**`streaming-zipformer-small-bilingual-zh-en`（约 50 MB）看起来是最省体积的中英双语选择，但它是在线模型，
这套接口用不了。** 离线且中英双语的现实候选是 SenseVoice 与 Whisper，**最后选了 Whisper**：

| 模型 | 许可 | 标点 | int8 文件合计 |
|---|---|---|---|
| **Whisper `base`** | **MIT**，可自由再分发 | ✅ 自带 | 153 MB |
| **Whisper `small`** | 同上 | ✅ 自带 | 358 MB |
| SenseVoice | FunASR 自定义协议、**非 OSI 认证** | ✅ | 228 MB |

**为什么放弃 SenseVoice**：它中文更强、体积也更小，但权重许可是阿里自定义的 FunASR Model Open Source
License Agreement —— 不是 OSI 认证的许可证，带署名与命名要求，还有一条「违反即授权终止」的行为条款；
社区转出来的 ONNX 转换件再分发条款也不明确。对一个 GPL-3.0、且因许可问题彻底移除过雾凇拼音的项目，
这个风险不该背。**Whisper 的代码与权重都是 MIT**，只需保留版权声明。

原先还担心「Whisper 没有标点、不适合直接上屏」—— **实测是错的**，它自带标点与大小写，见下面 spike 结果。

接口是**同步**的，线程约定由 Core 的 `VoiceWorker` 负责 —— 这与 `SentenceScorer` 同构，而不是 `Predictor`
那种「实现自己保证非阻塞」：识别不联网、不防抖、没有「最新请求优先」的语义。
`OfflineRecognizer` 在 crate 里已经 `unsafe impl Send + Sync`，不用自己写。

## 不做的事

- **不做流式 partial 实时显示**：trait 里因此不留 `partial()` 的口子；将来要加是默认方法，不破坏现有实现。
- **不做唤醒词常驻监听**。
- **不做云端 ASR**：与「本地」这个需求本身冲突。
- **不用 `.qj` 容器装语音模型**：sherpa-onnx 的配置只吃**文件路径**，装进容器还得先解出来，反而多一步。
- **不在 TSF DLL 里采音**：DLL 跑在每个应用进程内，受 AppContainer 限制拿不到麦克风，且不能带 Engine 的依赖树。
- **不做 pre-roll 与 VAD 尾部裁剪**：留到真机上听效果再定，这些取舍会在 CER / RTF 报告里显形。

## 构建依赖：一条要盯住的成本

`sherpa-onnx-sys` 的 `build.rs` 在没设 `SHERPA_ONNX_LIB_DIR` / `SHERPA_ONNX_ARCHIVE_DIR` 时，
会按目标三元组**从 GitHub Releases 下预编译静态库**。Windows x64 那一份是 **123 MB**，不保证快。

两个后果都实测过：

1. **它会打破跨平台 check**。官方只出 MSVC 产物（静态 CRT `/MT`），没有 windows-gnu 包，而
   **build script 在 `cargo check` 下照样会跑**——「反正是 check 不链接」不成立。
   所以依赖做成可选：`qingjian-voice` 的 `default = []`，`sherpa` feature 打开真正的后端；
   没编译时 `load_recognizer` 返回 `VoiceError::BackendNotCompiled`，CLI 与各壳**一行 `#[cfg]` 都不用写**。
2. **离线自编译的人会卡在下载上**。`SHERPA_ONNX_ARCHIVE_DIR` 可以指向一个放着预下载归档的目录
   （文件名要一字不差），CI 侧靠缓存 `target/`（`target/sherpa-onnx-prebuilt/` 在里面）避免每次重下。

日常命令，跨平台 check 一律绿、不下任何东西：

```bash
cargo check --target x86_64-pc-windows-gnu          # 照旧
cargo run -p qingjian-cli --features voice -- --voice-model <目录> --voice-wav <wav>
```

## spike 结果（2026-09-19）

8 核机器，`apps/cli --voice-wav` / `--eval-voice`，release 构建。
准确率用包自带的英文测试集（2 条、《红字》选段、289 字参考）。

| 模型 | int8 体积 | RTF（4 线程） | 英文 CER |
|---|---|---|---|
| **Whisper `base`** | **153 MB** | **0.261** | **0.35%** |
| Whisper `small` | 358 MB | 0.810 | 0.35% |

**线程数**（base）：2 → 0.325、**4 → 0.261**、8 → 0.356。调到核数以上反而更慢（争抢）。
`BackendConfig` 的缺省值按这个改成 4（`DEFAULT_THREADS`）。
debug 与 release 的 RTF 差在 5% 以内 —— 重活都在 sherpa-onnx 链接的**预编译 release 静态库**里。

### 结论

1. **`small` 不该提供**：慢 3.4 倍、大 2.4 倍，实测准确率一模一样。**「两档」这个设想在这份数据上不成立**
   —— 除非中文上能测出差距（见下面「还没测的」），否则第二档没有存在理由。
2. **标点与大小写自带**，且逐字准确（唯一一处「错」是 `dishonoured`/`dishonored` 的英美拼写变体）。
   这推翻了调研阶段的判断 —— 原先以为 Whisper 没有标点、因而不适合「直接上屏」，
   那正是当时偏向 SenseVoice 的理由之一。实测它恰恰很适合。
3. **延迟定下产品形态**：RTF 0.26 意味着说 10 秒要等约 2.6 秒，所以**定位为短句输入**。
   另一条路是用 VAD 检测停顿、边说边分段上屏，长段落的等待感基本消失 ——
   但那会把「整段一次出结果」这个简单形态拆掉，且要重新引入第一阶段刻意没做的流式，暂不走。

### 还没测的（重要）

- **中文与中英混说**。包自带的 `test_wavs` **全是英文**，中文 CER 必须用自己录的、带参考答案的音频集。
  **在拿到中文数字之前，模型选型只算定了一半** —— 中文才是这个输入法的主场景，
  而 Whisper 各尺寸在中文上的差距通常比英文上大得多，「small 不值」这个结论**有可能在中文上翻转**。
- **样本太小**：2 条、23 秒、朗读体（最容易的一类语音）。这份数据只能证明链路通、量级对，
  不足以支撑最终选型。
- **`tiny` 没测**。如果中文上 base 也不够快或不够准，`tiny`（更小更快）是下一个要看的方向。

## 与既有计划的关系

- 属 roadmap 的「其他输入方案」这一族，与五笔、注音并列的**输入来源**扩展，但不像它们那样改按键解析，
  而是加一路独立的输入通道。
- 平台接入顺序与五笔一致：先 Windows（采集放 Server 进程，触发键走现有按键流，**不需要改 IPC 协议**），
  再 macOS（采集在 Host 进程，补 `Info.plist` 的 `NSMicrophoneUsageDescription`）。
- 模型分发沿用 `tools/release/` 的数据管线（`data.lock` 记 SHA256 → `data-fetch.sh` 校验 → `release.yml`），
  与 `model.qjm` 同一套。
