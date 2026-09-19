//! 后台下载线程：逐文件下载、边下边报进度、校验 SHA256、全部成功才落盘。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use sha2::{Digest, Sha256};

use super::{Asset, Tier};
use crate::error::VoiceError;

/// 下载过程中报给调用方的事件。壳在自己的定时器里 [`Download::poll`] 取。
pub enum FetchEvent {
    /// 进度：已经下了多少字节 / 一共多少字节。分块到达时频繁报，壳做节流。
    Progress { done: u64, total: u64 },

    /// 下完并全部校验通过，模型落在哪个目录。
    Done { dir: PathBuf },

    /// 这次没成（网络、校验、写盘、被取消），消息里带着原因。临时目录已经清掉，可以重来。
    Failed(VoiceError),
}

/// 一次模型下载。
pub struct Download {
    events: Receiver<FetchEvent>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Download {
    /// 起一个后台线程把这一档模型下到 `dir`。
    ///
    /// 全程下在 `dir` 旁边的临时目录里，**全部文件都校验通过才改名成 `dir`** ——
    /// 中途失败、断网、被杀掉都不会留下半份模型让识别器去加载。
    pub fn spawn(tier: &Tier, dir: PathBuf) -> Self {
        let tier = tier.clone();
        let (events, receiver) = channel();
        let notice = events.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let handle = std::thread::Builder::new()
            .name("qingjian-voice-fetch".to_owned())
            .spawn(move || {
                if let Err(error) = run(&tier, &dir, &events, &flag) {
                    let _ = events.send(FetchEvent::Failed(error));
                }
            })
            .ok();
        if handle.is_none() {
            let _ = notice.send(FetchEvent::Failed(VoiceError::Fetch(
                "起不了下载线程".to_owned(),
            )));
        }
        Self {
            events: receiver,
            cancel,
            handle,
        }
    }

    /// 取一条事件；没有就 `None`。非阻塞。
    pub fn poll(&self) -> Option<FetchEvent> {
        self.events.try_recv().ok()
    }

    /// 取消。线程在下一个分块边界退出并清掉临时目录，正式目录不受影响。
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// 后台线程跑完了没有。正常结束前一定先发过 [`FetchEvent::Done`] 或 [`FetchEvent::Failed`]，
    /// 所以「已结束但 [`Self::poll`] 取不到事件」只可能是线程意外死了 —— 调用方据此别死等。
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        // 结构没了就没人要结果了，让线程尽快收手；不等它（可能正卡在一次网络读上）
        self.cancel();
        let _ = self.handle.take();
    }
}

/// 下载线程的主体。
fn run(
    tier: &Tier,
    dir: &Path,
    events: &Sender<FetchEvent>,
    cancel: &AtomicBool,
) -> Result<(), VoiceError> {
    let total = tier.total_bytes();
    let staging = staging_dir(dir);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| VoiceError::Fetch(format!("建下载运行时失败：{error}")))?;
    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| VoiceError::Fetch(format!("建 HTTP 客户端失败：{error}")))?;

    let mut done: u64 = 0;
    let outcome = (|| {
        for asset in &tier.files {
            let path = staging.join(&asset.name);
            let written = runtime.block_on(fetch_one(
                &client, asset, &path, events, done, total, cancel,
            ))?;
            done += written;
            let _ = events.send(FetchEvent::Progress { done, total });
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    promote(&staging, dir)?;
    let _ = events.send(FetchEvent::Done {
        dir: dir.to_path_buf(),
    });
    Ok(())
}

/// 下一个文件：边下边报进度，下完核对 SHA256，返回实际下了多少字节。
async fn fetch_one(
    client: &reqwest::Client,
    asset: &Asset,
    path: &Path,
    events: &Sender<FetchEvent>,
    already: u64,
    total: u64,
    cancel: &AtomicBool,
) -> Result<u64, VoiceError> {
    let mut response = client
        .get(&asset.url)
        .send()
        .await
        .map_err(|error| VoiceError::Fetch(format!("{} 请求失败：{error}", asset.name)))?
        .error_for_status()
        .map_err(|error| VoiceError::Fetch(format!("{} 返回错误状态：{error}", asset.name)))?;

    let mut file = std::fs::File::create(path)?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| VoiceError::Fetch(format!("{} 读响应失败：{error}", asset.name)))?
    {
        if cancel.load(Ordering::SeqCst) {
            return Err(VoiceError::Cancelled);
        }
        file.write_all(&chunk)?;
        hasher.update(&chunk);
        written += chunk.len() as u64;
        let _ = events.send(FetchEvent::Progress {
            done: already + written,
            total,
        });
    }
    file.flush()?;
    drop(file);

    let actual = hex(&hasher.finalize());
    if actual != asset.sha256.to_ascii_lowercase() {
        return Err(VoiceError::Fetch(format!(
            "{} 校验不过：清单写的是 {}，下下来的是 {actual}",
            asset.name, asset.sha256
        )));
    }
    Ok(written)
}

/// 把下好的临时目录换成正式目录。
///
/// 先把旧的挪开再改名、最后删旧的：这样任何一刻磁盘上都有一份完整模型（或一份都没有），
/// 不会出现「正式目录在、但里面是半份」的状态。
fn promote(staging: &Path, dir: &Path) -> Result<(), VoiceError> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let backup = dir.with_file_name(format!(
        "{}.old",
        dir.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model")
    ));
    let _ = std::fs::remove_dir_all(&backup);
    if dir.exists() {
        std::fs::rename(dir, &backup)?;
    }
    match std::fs::rename(staging, dir) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(error) => {
            // 改名没成，把旧的挪回去，别让用户连原来那档也没了
            if backup.exists() {
                let _ = std::fs::rename(&backup, dir);
            }
            Err(VoiceError::Store(error))
        }
    }
}

/// 临时目录：与正式目录同级，改名才是同分区内的操作。
fn staging_dir(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model");
    dir.with_file_name(format!(".{name}.tmp"))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, Write as _};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    use super::*;

    /// 一个只回一条固定响应的迷你 HTTP 服务：把下载链路在本地跑通，不联网也不要真模型。
    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                line.clear();
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{address}/asset")
    }

    fn tier_of(url: String, sha256: String, size: u64) -> Tier {
        Tier {
            label: "测试档".to_owned(),
            note: "测试用".to_owned(),
            files: vec![Asset {
                name: "model.onnx".to_owned(),
                url,
                sha256,
                size,
            }],
        }
    }

    /// 一个各次运行都不重样的临时目录。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qingjian-voice-fetch-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 等到结果事件为止。
    fn wait(download: &Download) -> FetchEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match download.poll() {
                Some(FetchEvent::Progress { .. }) => {}
                Some(event) => return event,
                None => assert!(Instant::now() < deadline, "下载没结束"),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn downloads_verifies_and_promotes() {
        let body = b"not really a model, but it has a checksum".to_vec();
        let sha = hex(&Sha256::digest(&body));
        let url = serve_once(body.clone());
        let dir = scratch("ok");

        let download = Download::spawn(&tier_of(url, sha, body.len() as u64), dir.clone());
        let FetchEvent::Done { dir: reported } = wait(&download) else {
            panic!("expected the download to succeed");
        };
        assert_eq!(reported, dir);
        assert_eq!(std::fs::read(dir.join("model.onnx")).unwrap(), body);
        // 临时目录不该留下
        assert!(!staging_dir(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_checksum_mismatch_leaves_nothing_behind() {
        let body = b"the wrong bytes".to_vec();
        let url = serve_once(body.clone());
        let dir = scratch("mismatch");
        // 故意给一个对不上的校验值，模拟「下载被中间人改过 / CDN 给了旧文件」
        let wrong = hex(&Sha256::digest(b"something else"));

        let download = Download::spawn(&tier_of(url, wrong, body.len() as u64), dir.clone());
        let FetchEvent::Failed(error) = wait(&download) else {
            panic!("expected the download to fail");
        };
        assert!(matches!(error, VoiceError::Fetch(_)), "应当是校验不过");
        // 正式目录不能被建出来，临时目录要清掉 —— 绝不能让识别器去加载半份模型
        assert!(!dir.exists());
        assert!(!staging_dir(&dir).exists());
    }

    #[test]
    fn a_stale_model_directory_survives_a_failed_download() {
        let body = b"the wrong bytes".to_vec();
        let url = serve_once(body.clone());
        let dir = scratch("stale");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model.onnx"), b"the model that already works").unwrap();

        let wrong = hex(&Sha256::digest(b"something else"));
        let download = Download::spawn(&tier_of(url, wrong, body.len() as u64), dir.clone());
        assert!(matches!(wait(&download), FetchEvent::Failed(_)));

        // 下载失败不该动用户已经装好的那一份
        assert_eq!(
            std::fs::read(dir.join("model.onnx")).unwrap(),
            b"the model that already works"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
