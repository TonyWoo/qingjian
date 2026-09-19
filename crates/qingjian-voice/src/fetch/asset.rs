//! 清单里的一个文件：落到磁盘上的名字、从哪下、校验值、多大。

use serde::Deserialize;

/// 一个要下载的模型文件。清单里一个文件一条。
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    /// 落到模型目录里的文件名（sherpa 的后端是按文件名认模型家族的，不能改名）。
    pub name: String,

    /// 下载地址。
    pub url: String,

    /// 小写十六进制的 SHA256：下完必须对得上，对不上就当这次下载失败。
    pub sha256: String,

    /// 字节数。只用来算总进度与界面上的「约多少 MB」，不作校验。
    pub size: u64,
}
