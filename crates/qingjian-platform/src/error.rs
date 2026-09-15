use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write config {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid config {path}: {source}")]
    Parse {
        path: PathBuf,

        /// Box 起来：`toml::de::Error` 本身 88 字节，直接放进枚举会把 `ConfigError` 顶到 128 字节以上，
        /// 于是每个返回它的函数都撞 clippy 的 `result_large_err`（MSVC 的枚举布局比 Itanium ABI 更占地方，
        /// 所以 Linux / macOS 上不报、Windows 上报）。
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("cannot edit config {path} in place: {source}")]
    Edit {
        path: PathBuf,

        /// 同上。
        #[source]
        source: Box<toml_edit::TomlError>,
    },
}
