//! Streaming model package downloads. Only complete, validated archives replace
//! installed files; cancellation or network errors leave the old model intact.

use std::fs::{self, File};
use std::future::Future;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use reqwest::Client;

use crate::virtual_path::normalize_member;

/// Same model source as BANDORI-PET-REV's ModelPackageDownloadWorker.
pub const MODEL_PACKAGE_BASE_URL: &str =
    "https://modelscope.cn/datasets/HELPMEEADICE/BanG-Dream-Live2D/resolve/master/models";

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("下载已取消")]
    Cancelled,
    #[error("无效的角色标识：{0}")]
    InvalidKey(String),
    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),
    #[error("文件读写失败：{0}")]
    Io(#[from] io::Error),
    #[error("模型包校验失败：{0}")]
    InvalidArchive(String),
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub bytes_per_second: f64,
}

pub struct PackageDownloader {
    client: Client,
    base_url: String,
    runtime: tokio::runtime::Runtime,
}

impl PackageDownloader {
    pub fn new() -> Result<Self, DownloadError> {
        Self::with_base_url(MODEL_PACKAGE_BASE_URL)
    }

    fn with_base_url(base_url: &str) -> Result<Self, DownloadError> {
        Ok(Self {
            client: Client::builder()
                .user_agent("Bandori-Pet/1.0")
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(30))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_owned(),
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        })
    }

    /// Always fetches the latest package, also used for explicit updates.
    pub fn download(
        &self,
        key: &str,
        models_dir: &Path,
        cancelled: &AtomicBool,
        mut progress: impl FnMut(DownloadProgress),
    ) -> Result<PathBuf, DownloadError> {
        self.runtime.block_on(async {
            if key.is_empty()
                || key.starts_with('.')
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err(DownloadError::InvalidKey(key.into()));
            }
            check_cancelled(cancelled)?;
            fs::create_dir_all(models_dir)?;
            let target = models_dir.join(format!("{key}.zst"));
            let request = self
                .client
                .get(format!("{}/{key}.zst", self.base_url))
                .send();
            let mut response = cancellable(request, cancelled).await?.error_for_status()?;
            let total = response.content_length().filter(|size| *size > 0);
            // Unique temporary files also prevent two UI instances sharing a .part.
            let mut partial = tempfile::Builder::new()
                .prefix(&format!(".{key}-"))
                .suffix(".part")
                .tempfile_in(models_dir)?;
            let started = Instant::now();
            let mut last_report = started;
            let mut downloaded = 0;
            loop {
                check_cancelled(cancelled)?;
                let Some(chunk) = cancellable(response.chunk(), cancelled).await? else {
                    break;
                };
                check_cancelled(cancelled)?;
                partial.write_all(&chunk)?;
                downloaded += chunk.len() as u64;
                if last_report.elapsed() >= Duration::from_millis(100) {
                    progress(DownloadProgress {
                        downloaded,
                        total,
                        bytes_per_second: downloaded as f64
                            / started.elapsed().as_secs_f64().max(0.001),
                    });
                    last_report = Instant::now();
                }
            }
            if downloaded == 0 || total.is_some_and(|size| size != downloaded) {
                return Err(DownloadError::InvalidArchive("响应为空或下载不完整".into()));
            }
            progress(DownloadProgress {
                downloaded,
                total,
                bytes_per_second: 0.0,
            });
            partial.flush()?;
            validate_archive(partial.path(), cancelled)?;
            check_cancelled(cancelled)?;
            partial.as_file().sync_all()?;
            // NamedTempFile::persist atomically replaces an existing file on Windows
            // too, with no delete-before-rename gap that could lose the old model.
            partial.persist(&target).map_err(|error| error.error)?;
            Ok(target)
        })
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), DownloadError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(DownloadError::Cancelled)
    } else {
        Ok(())
    }
}

/// Read the entire stream rather than trusting the embedded member index. This
/// catches truncated transfers, checksum errors and HTML error responses.
fn validate_archive(path: &Path, cancelled: &AtomicBool) -> Result<(), DownloadError> {
    let mut decoder = zstd::stream::read::Decoder::new(File::open(path)?)?;
    let mut archive = tar::Archive::new(&mut decoder);
    let mut found = false;
    for entry in archive.entries()? {
        check_cancelled(cancelled)?;
        let mut entry = entry?;
        let member = entry.path()?.to_string_lossy().replace('\\', "/");
        if member.starts_with('/') || member.contains(':') {
            return Err(DownloadError::InvalidArchive("模型包包含绝对路径".into()));
        }
        normalize_member(&member)
            .map_err(|error| DownloadError::InvalidArchive(error.to_string()))?;
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err(DownloadError::InvalidArchive("模型包包含链接".into()));
        }
        if kind.is_file() {
            if let Some((costume, file)) = member.split_once('/') {
                found |= !costume.is_empty()
                    && !file.contains('/')
                    && (file.eq_ignore_ascii_case("model.json")
                        || file.to_ascii_lowercase().ends_with(".model3.json"));
            }
            let mut buffer = [0u8; 64 * 1024];
            while entry.read(&mut buffer)? != 0 {
                check_cancelled(cancelled)?;
            }
        }
    }
    drop(archive);
    let mut buffer = [0u8; 64 * 1024];
    while decoder.read(&mut buffer)? != 0 {
        check_cancelled(cancelled)?;
    }
    if !found {
        return Err(DownloadError::InvalidArchive(
            "未找到可用的 model.json 或 .model3.json".into(),
        ));
    }
    Ok(())
}

// Poll the same pending future so cancellation also works while waiting for
// headers or when the server stops sending data.
async fn cancellable<T>(
    future: impl Future<Output = Result<T, reqwest::Error>>,
    cancelled: &AtomicBool,
) -> Result<T, DownloadError> {
    let mut pending = std::pin::pin!(future);
    loop {
        check_cancelled(cancelled)?;
        if let Ok(result) = tokio::time::timeout(Duration::from_millis(100), pending.as_mut()).await
        {
            check_cancelled(cancelled)?;
            return Ok(result?);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    fn model_archive() -> Vec<u8> {
        let encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(2);
        header.set_cksum();
        tar.append_data(&mut header, "casual/model.json", b"{}".as_slice())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn server(body: Vec<u8>, status: u16, declared_extra: usize) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte).unwrap_or(0) == 0 {
                    break;
                }
                request.push(byte[0]);
            }
            let header = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len() + declared_extra
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }

    fn assert_no_partials(models: &Path) {
        assert!(fs::read_dir(models).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part")));
    }

    #[test]
    fn downloads_and_atomically_updates_existing_package() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        let body = model_archive();
        for _ in 0..2 {
            let (url, handle) = server(body.clone(), 200, 0);
            let downloader = PackageDownloader::with_base_url(&url).unwrap();
            let path = downloader
                .download("kasumi", &models, &AtomicBool::new(false), |_| {})
                .unwrap();
            assert_eq!(fs::read(path).unwrap(), body);
            let request = handle.join().unwrap().to_lowercase();
            assert!(request.starts_with("get /models/kasumi.zst "));
            assert!(request.contains("user-agent: bandori-pet/1.0"));
            assert_no_partials(&models);
            assert_eq!(
                crate::index::load_or_scan(&models)
                    .costumes_of("kasumi")
                    .len(),
                1
            );
        }
    }

    #[test]
    fn failed_updates_keep_the_old_model_and_remove_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("kasumi.zst");
        fs::write(&target, b"previous model").unwrap();
        let mut truncated = model_archive();
        truncated.truncate(truncated.len() - 8);
        for (body, status, extra) in [
            (b"<html>error</html>".to_vec(), 200, 0),
            (model_archive(), 200, 100),
            (b"not found".to_vec(), 404, 0),
            (truncated, 200, 0),
            (Vec::new(), 200, 0),
        ] {
            let (url, handle) = server(body, status, extra);
            let result = PackageDownloader::with_base_url(&url).unwrap().download(
                "kasumi",
                dir.path(),
                &AtomicBool::new(false),
                |_| {},
            );
            assert!(result.is_err());
            handle.join().unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"previous model");
            assert_no_partials(dir.path());
        }
    }

    #[test]
    fn cancellation_before_install_preserves_existing_model() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("kasumi.zst"), b"old").unwrap();
        let (url, handle) = server(model_archive(), 200, 0);
        let cancelled = AtomicBool::new(false);
        let result = PackageDownloader::with_base_url(&url).unwrap().download(
            "kasumi",
            dir.path(),
            &cancelled,
            |_| cancelled.store(true, Ordering::Relaxed),
        );
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        handle.join().unwrap();
        assert_eq!(fs::read(dir.path().join("kasumi.zst")).unwrap(), b"old");
        assert_no_partials(dir.path());
    }

    #[test]
    fn rejects_unsafe_package_keys_without_network_or_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = PackageDownloader::with_base_url("http://127.0.0.1:1").unwrap();
        for key in [
            "",
            "../kasumi",
            "C:/kasumi",
            "a/b",
            "a\\b",
            ".hidden",
            "a?x=1",
        ] {
            assert!(matches!(
                downloader.download(key, dir.path(), &AtomicBool::new(false), |_| {}),
                Err(DownloadError::InvalidKey(_))
            ));
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn can_cancel_while_server_stops_sending_data() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (connected, wait_connected) = std::sync::mpsc::channel();
        let (finished, wait_finished) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte).unwrap_or(0) == 0 {
                    return;
                }
                request.push(byte[0]);
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 999999\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            connected.send(()).unwrap();
            let _ = wait_finished.recv_timeout(Duration::from_secs(5));
        });
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let trigger = std::thread::spawn(move || {
            wait_connected.recv_timeout(Duration::from_secs(5)).unwrap();
            flag.store(true, Ordering::Relaxed);
        });
        let dir = tempfile::tempdir().unwrap();
        let downloader = PackageDownloader::with_base_url(&url).unwrap();
        let started = Instant::now();
        let result = downloader.download("kasumi", dir.path(), &cancelled, |_| {});
        assert!(
            matches!(result, Err(DownloadError::Cancelled)),
            "unexpected result: {result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        finished.send(()).unwrap();
        trigger.join().unwrap();
        server.join().unwrap();
        assert_no_partials(dir.path());
    }

    #[test]
    #[ignore = "downloads a 42 MB public model to a temporary directory"]
    fn downloads_public_model_and_discovers_its_costumes() {
        let dir = tempfile::tempdir().unwrap();
        PackageDownloader::new()
            .unwrap()
            .download("kasumi", dir.path(), &AtomicBool::new(false), |_| {})
            .unwrap();
        let index = crate::index::load_or_scan(dir.path());
        assert!(!index.costumes_of("kasumi").is_empty());
        assert_no_partials(dir.path());
        eprintln!(
            "public model validated: {} costumes",
            index.costumes_of("kasumi").len()
        );
    }
}
