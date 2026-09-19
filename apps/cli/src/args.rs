use std::path::PathBuf;

use clap::Parser;

/// 按优先级挑一个存在的数据文件：`data/generated/` 里打包好的 `.qj`、那里的 TSV、仓库自带的产品数据
/// （`assets/lexicon/dict.tsv`、`assets/glossary/glossary-*.tsv`、`assets/lexicon/english.tsv`），最后是 `assets/sample/` 的样例。
pub fn default_data_file(name: &str) -> PathBuf {
    let generated = PathBuf::from("data/generated").join(name);
    let packed = generated.with_extension("qj");
    let shipped = if name.starts_with("glossary-") {
        PathBuf::from("assets/glossary").join(name)
    } else {
        PathBuf::from("assets/lexicon").join(name)
    };
    for candidate in [packed, generated, shipped] {
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("assets/sample").join(name)
}

/// 缺省配置文件位置：与输入法共用同一份。
pub fn default_config_file() -> PathBuf {
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join("Library/Application Support/Qingjian/config.toml");
    }
    PathBuf::from("config.toml")
}

#[derive(Debug, Parser)]
#[command(name = "qingjian", about = "青简输入法 Core 测试工具")]
pub struct Args {
    /// 词库路径（TSV）。缺省：data/generated/dict.tsv 存在就用它，否则 assets/sample/dict.tsv
    #[arg(long)]
    pub dict: Option<PathBuf>,

    /// 释义表路径。缺省：data/generated/glossary-<language>.tsv 存在就用它，否则 assets/sample/ 下的同名文件
    #[arg(long)]
    pub glossary: Option<PathBuf>,

    /// 学习语言：en / ja / es。也可用环境变量 QINGJIAN_LEARNING_LANGUAGE
    #[arg(long, env = "QINGJIAN_LEARNING_LANGUAGE", default_value = "en")]
    pub language: String,

    /// 附加词库（.qj 或 TSV），可给多个，与主词库一起查
    #[arg(long)]
    pub extra_dict: Vec<PathBuf>,

    /// 英文词表路径（中英混输）。缺省：data/generated/english.tsv 存在就用它，否则不启用
    #[arg(long)]
    pub english: Option<PathBuf>,

    /// 用户词频文件；给了就在退出时写回，不给则只在本次会话内学习
    #[arg(long)]
    pub user_dict: Option<PathBuf>,

    /// 配置文件路径。缺省：~/Library/Application Support/Qingjian/config.toml（macOS）或 ./config.toml
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// 启用云联想（无视配置里的 enabled）；密钥来自配置或 QINGJIAN_API_KEY（`api_key_env`）
    #[arg(long)]
    pub predict: bool,

    /// 模糊音，逗号分隔（z-zh,c-ch,s-sh,n-l,f-h,l-r,an-ang,en-eng,in-ing），`all` 全开；给了就覆盖配置里的 [fuzzy]
    #[arg(long, value_delimiter = ',')]
    pub fuzzy: Vec<String>,

    /// 英文模式（输入法里是 Caps Lock 亮着）：字母不当拼音，候选来自英文词表的补全与拼错纠正
    #[arg(long)]
    pub english_mode: bool,

    /// 打开中文优先（配置 [general] chinese_first = true）：整段是英文词时中文候选排第一、英文第二，评测两种排法用
    #[arg(long)]
    pub chinese_first: bool,

    /// 双拼方案（xiaohe / ziranma / microsoft / sogou），覆盖配置里的 [general] shuangpin；off 强制全拼
    #[arg(long)]
    pub shuangpin: Option<String>,

    /// 形码码表（五笔）的 TSV 文件（`词\t编码\t词频`）：给了就用编码查表，不走拼音那一套
    #[arg(long, value_name = "码表")]
    pub wubi: Option<PathBuf>,

    /// 神经重打分：字级 Transformer 的 .qjm 文件或导出目录（model.safetensors / config.json / vocab.json），整句前几条路径用它重排
    #[arg(long)]
    pub neural: Option<PathBuf>,

    /// 神经重打分的权重 λ（0 到 1，缺省 0.5）：最终分 = 路径分 + λ·(神经分 − 静态二元分)，个人学习与代价不受影响
    #[arg(long)]
    pub neural_weight: Option<f64>,

    /// 神经重打分的门槛（nat，缺省不设）：路径分落后最优路径超过这么多的不参与重排
    #[arg(long)]
    pub neural_margin: Option<f64>,

    /// 神经重打分给模型看的前文字符数（缺省 64，0 为不给前文）
    #[arg(long)]
    pub neural_context: Option<usize>,

    /// 神经重打分走后台线程（输入法壳里的接法）：查询先按词级模型出候选，再请求 / 等待重打分后重查一次；结果应与同步一致
    #[arg(long)]
    pub neural_async: bool,

    /// 下载一档语音识别模型，下完打印落到的目录后退出。档位名见 voice.lock；
    /// 给不认识的名字会列出可选的。不需要引擎，也不碰任何界面
    #[arg(long, value_name = "档位")]
    pub voice_fetch: Option<String>,

    /// 语音模型下到哪。缺省 data/voice（各壳会换成自己的用户数据目录）
    #[arg(long, value_name = "目录")]
    pub voice_dir: Option<PathBuf>,

    /// 本地语音识别模型目录（sherpa-onnx 导出的那一份）。这个二进制要带 `--features voice` 编译
    /// 才认得出模型，否则报「后端没编译进来」
    #[arg(long, value_name = "目录")]
    pub voice_model: Option<PathBuf>,

    /// 语音识别的推理线程数（缺省 4，实测最优；调到核数以上反而更慢）
    #[arg(long, default_value_t = qingjian_voice::DEFAULT_THREADS)]
    pub voice_threads: i32,

    /// 用这个 WAV 走一遍完整语音输入（开始 → 喂样本 → 结束 → 上屏），打印文本与耗时后退出。
    /// 只收 16 kHz 单声道 WAV
    #[arg(long, value_name = "WAV")]
    pub voice_wav: Option<PathBuf>,

    /// 语音识别评测：目录里每对 `<名字>.wav` + `<名字>.txt`（UTF-8 参考答案）算一条，
    /// 出 CER 与 RTF 报告；可给多个目录
    #[arg(long, value_name = "目录", num_args = 1..)]
    pub eval_voice: Vec<PathBuf>,

    /// 语音评测报告里列前 N 条最差的例子
    #[arg(long, default_value_t = 10)]
    pub voice_misses: usize,

    /// 逐键模式：把每个输入当作一键一键敲进去，每个前缀都查一次，打印每键各阶段耗时（性能测试用）
    #[arg(long)]
    pub typing: bool,

    /// 只显示前 N 个候选
    #[arg(long, default_value_t = 9)]
    pub limit: usize,

    /// 回放评测：读输入日志（input-log.jsonl），把每次上屏时的键重新喂给引擎，算首选命中率等指标。只在内存里学习，不写任何文件
    #[arg(long)]
    pub replay: Option<PathBuf>,

    /// 回放 / 整句评测时打印前 N 条没命中首选的例子
    #[arg(long, default_value_t = 20)]
    pub misses: usize,

    /// 覆盖引擎里的调参常数，`名=值`，逗号分隔或多次给。名字：lambda / k / cap / discount（个人 n-gram 插值 λ / K / 封顶 / 三元折扣），
    /// transpose / substitute / extra / missing / typo-cap / correction（敲错四类代价 / 个人折扣上限 / 整段纠错代价）
    #[arg(long, value_delimiter = ',')]
    pub tune: Vec<String>,

    /// 整句评测：读中文文本（一行一段，按标点切句、按词库转成全拼）或 `--eval-save` 冻结下来的三列文件，
    /// 冷启动喂给引擎看整句能不能还原原句；可给多个文件
    #[arg(long, num_args = 1..)]
    pub eval_text: Vec<PathBuf>,

    /// 把整句评测用到的句子集写成 `句子\t拼音\t上文` 三列文件，下次直接 `--eval-text` 它，保证比的是同一份句子
    #[arg(long)]
    pub eval_save: Option<PathBuf>,

    /// 直接查询这些拼音后退出；不给则进入交互模式
    pub inputs: Vec<String>,
}
