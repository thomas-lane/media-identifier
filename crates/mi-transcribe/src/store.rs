//! Downloading and verifying model files.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use mi_types::{CancelFlag, ModelInfo, ModelState, ModelStatus, SpeechModel};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::catalog::{PinnedFile, model_info, vad_model_file};
use crate::{Result, TranscribeError};

/// `User-Agent` sent with every download.
pub const USER_AGENT: &str = concat!("MediaIdentifier/", env!("CARGO_PKG_VERSION"));

/// How often progress is reported while downloading.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// How often a download checks the cancellation flag while waiting for data.
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// Speed is averaged over this much recent time, so the estimate follows changes without
/// jumping on every chunk.
const SPEED_WINDOW: Duration = Duration::from_secs(3);
/// A request that delivers no new bytes this many times in a row fails the download.
const MAX_ATTEMPTS_WITHOUT_PROGRESS: u32 = 3;
/// A connection that sends nothing for this long is dropped (and retried).
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// The folder holding model files (`<app data>/models`).
///
/// A download writes `<file>.part` and resumes it with an HTTP `Range` request; the finished file
/// is hashed and renamed to its final name only when size and SHA-256 match the pinned values in
/// [`crate::catalog`], so a file under its final name is always complete and verified.
///
/// Each speech model is downloaded together with the voice activity detection model
/// ([`vad_model_file`]); a model counts as ready only when both files are present.
#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
}

/// Progress of one file, before it is mapped to a [`ModelState`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Downloading {
        downloaded: u64,
        total: u64,
        bytes_per_second: f64,
    },
    Verifying,
}

/// Recent download speed over [`SPEED_WINDOW`].
#[derive(Debug, Default)]
struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
}

impl SpeedMeter {
    fn record(&mut self, now: Instant, downloaded: u64) -> f64 {
        self.samples.push_back((now, downloaded));
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > SPEED_WINDOW && self.samples.len() > 2 {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        match (self.samples.front(), self.samples.back()) {
            (Some(&(t0, b0)), Some(&(t1, b1))) if t1 > t0 => {
                (b1 - b0) as f64 / t1.duration_since(t0).as_secs_f64()
            }
            _ => 0.0,
        }
    }
}

fn download_error(message: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Download(message.to_string())
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        // Byte ranges refer to the stored file, so the transfer must not be re-encoded.
        .no_gzip()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(download_error)
}

fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

fn part_path(final_path: &Path) -> PathBuf {
    let mut name = final_path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    final_path.with_file_name(name)
}

/// SHA-256 of a file as lowercase hex.
fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Parses the first byte position of a `Content-Range: bytes <start>-<end>/<total>` header.
fn content_range_start(response: &reqwest::Response) -> Option<u64> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    value
        .trim()
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

impl ModelStore {
    /// Uses `dir`, creating it on first download.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Final path of a model file (whether or not it exists).
    pub fn model_path(&self, model: SpeechModel) -> PathBuf {
        self.dir.join(model_info(model).file_name)
    }

    /// The voice activity detection model, when it has been downloaded.
    pub fn vad_model_path(&self) -> Option<PathBuf> {
        let file = vad_model_file();
        let path = self.dir.join(&file.file_name);
        (file_len(&path) == Some(file.size_bytes)).then_some(path)
    }

    /// Current state from disk, without hashing (that happens once, after download):
    ///
    /// - `Ready` when the model file and the voice activity detection model exist with their
    ///   pinned sizes.
    /// - `Paused` when a partial `.part` download of the model exists, or when the model is
    ///   complete but the small voice activity detection model is not (downloading again
    ///   fetches only that file).
    /// - `Missing` otherwise.
    pub fn status(&self, model: SpeechModel) -> ModelStatus {
        let info = model_info(model);
        let path = self.dir.join(&info.file_name);
        let total = info.size_bytes;
        let state = if file_len(&path) == Some(total) {
            if self.vad_model_path().is_some() {
                ModelState::Ready
            } else {
                ModelState::Paused {
                    downloaded: total,
                    total,
                }
            }
        } else if let Some(downloaded) = file_len(&part_path(&path)) {
            ModelState::Paused {
                downloaded: downloaded.min(total),
                total,
            }
        } else {
            ModelState::Missing
        };
        ModelStatus { info, state }
    }

    /// Downloads (or resumes) a model and the voice activity detection model, calling
    /// `on_progress` about every 250 ms with a `Downloading` status for the model, then
    /// `Verifying`, then `Ready`. Returns the model's final path.
    ///
    /// Sends `User-Agent: MediaIdentifier/<version>`. A connection that drops is resumed
    /// automatically from the bytes received; the download fails after three requests in a row
    /// deliver nothing. When `cancel` is set the `.part` file is kept, `on_progress` receives
    /// the state on disk (normally `Paused`) and the call returns `Cancelled`; the next call
    /// resumes. On any other error `on_progress` receives `Failed`. On a checksum mismatch the
    /// `.part` file is deleted, so the next attempt starts over.
    pub async fn download(
        &self,
        model: SpeechModel,
        on_progress: &(dyn Fn(ModelStatus) + Send + Sync),
        cancel: &CancelFlag,
    ) -> Result<PathBuf> {
        let info = model_info(model);
        let result = self
            .download_files(&info, &[vad_model_file()], on_progress, cancel)
            .await;
        if matches!(result, Err(TranscribeError::Cancelled)) {
            on_progress(self.status(model));
        }
        result
    }

    /// Downloads the file described by `info`, then each of `extra` (without progress reports),
    /// and reports the final state.
    async fn download_files(
        &self,
        info: &ModelInfo,
        extra: &[PinnedFile],
        on_progress: &(dyn Fn(ModelStatus) + Send + Sync),
        cancel: &CancelFlag,
    ) -> Result<PathBuf> {
        let emit = |state: ModelState| {
            on_progress(ModelStatus {
                info: info.clone(),
                state,
            })
        };
        let result = async {
            let client = http_client()?;
            let report = |phase: Phase| {
                emit(match phase {
                    Phase::Downloading {
                        downloaded,
                        total,
                        bytes_per_second,
                    } => ModelState::Downloading {
                        downloaded,
                        total,
                        bytes_per_second,
                    },
                    Phase::Verifying => ModelState::Verifying,
                })
            };
            let path = self
                .fetch(&client, &PinnedFile::from(info), &report, cancel)
                .await?;
            for file in extra {
                self.fetch(&client, file, &|_| {}, cancel).await?;
            }
            Ok(path)
        }
        .await;
        match &result {
            Ok(_) => emit(ModelState::Ready),
            Err(TranscribeError::Cancelled) => {}
            Err(error) => emit(ModelState::Failed {
                message: error.to_string(),
            }),
        }
        result
    }

    /// Downloads one pinned file into the folder (resuming a `.part` file), verifies it and moves
    /// it to its final name. Returns at once when the final file already exists with the pinned
    /// size.
    async fn fetch(
        &self,
        client: &reqwest::Client,
        file: &PinnedFile,
        report: &(dyn Fn(Phase) + Send + Sync),
        cancel: &CancelFlag,
    ) -> Result<PathBuf> {
        let final_path = self.dir.join(&file.file_name);
        match file_len(&final_path) {
            Some(len) if len == file.size_bytes => return Ok(final_path),
            // A file of the wrong size under the final name was not written by this store.
            Some(_) => tokio::fs::remove_file(&final_path).await?,
            None => {}
        }
        tokio::fs::create_dir_all(&self.dir).await?;
        let part = part_path(&final_path);
        if file_len(&part).is_some_and(|len| len > file.size_bytes) {
            tokio::fs::remove_file(&part).await?;
        }

        let mut attempts_without_progress = 0;
        let mut last_error = None;
        loop {
            if cancel.is_cancelled() {
                return Err(TranscribeError::Cancelled);
            }
            let offset = file_len(&part).unwrap_or(0);
            if offset == file.size_bytes {
                break;
            }
            if attempts_without_progress >= MAX_ATTEMPTS_WITHOUT_PROGRESS {
                return Err(last_error.unwrap_or_else(|| download_error("no data received")));
            }
            let (received, error) = match self
                .fetch_range(client, file, &part, offset, report, cancel)
                .await
            {
                Ok(received) => (received, None),
                Err((received, error)) => (received, Some(error)),
            };
            match error {
                Some(TranscribeError::Cancelled) => return Err(TranscribeError::Cancelled),
                Some(error @ TranscribeError::Io(_)) => return Err(error),
                Some(error) => {
                    tracing::warn!(file = %file.file_name, %error, "download attempt failed");
                    last_error = Some(error);
                }
                None => {}
            }
            if received > 0 {
                attempts_without_progress = 0;
            } else {
                attempts_without_progress += 1;
            }
        }

        report(Phase::Verifying);
        let hash_path = part.clone();
        let actual = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
            .await
            .map_err(|e| download_error(format!("verification stopped: {e}")))??;
        if actual != file.sha256 {
            tokio::fs::remove_file(&part).await?;
            return Err(TranscribeError::ChecksumMismatch {
                file: file.file_name.clone(),
                expected: file.sha256.clone(),
                actual,
            });
        }
        tokio::fs::rename(&part, &final_path).await?;
        Ok(final_path)
    }

    /// One HTTP request from `offset` to the end of the file, appending to `part`. Returns the
    /// number of bytes kept in `part`, also on error.
    async fn fetch_range(
        &self,
        client: &reqwest::Client,
        file: &PinnedFile,
        part: &Path,
        offset: u64,
        report: &(dyn Fn(Phase) + Send + Sync),
        cancel: &CancelFlag,
    ) -> std::result::Result<u64, (u64, TranscribeError)> {
        let mut request = client.get(&file.url);
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let response = tokio::select! {
            r = request.send() => r.map_err(|e| (0, download_error(e)))?,
            () = wait_for_cancel(cancel) => return Err((0, TranscribeError::Cancelled)),
        };
        let status = response.status();
        let start = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            match content_range_start(&response) {
                Some(start) if start == offset => offset,
                other => {
                    return Err((
                        0,
                        download_error(format!(
                            "server resumed at byte {other:?} instead of {offset}"
                        )),
                    ));
                }
            }
        } else if status.is_success() {
            // The server ignored the range and sent the whole file.
            0
        } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // The partial file does not fit the remote file; start over.
            let _ = tokio::fs::remove_file(part).await;
            return Err((0, download_error("server refused to resume; restarting")));
        } else {
            return Err((
                0,
                download_error(format!("HTTP {status} from {}", file.url)),
            ));
        };

        let io = |e: std::io::Error| (0, TranscribeError::Io(e));
        let mut out = if start == 0 {
            tokio::fs::File::create(part).await.map_err(io)?
        } else {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(part)
                .await
                .map_err(io)?
        };
        let mut downloaded = start;
        let mut received = 0u64;
        let mut meter = SpeedMeter::default();
        let mut last_report: Option<Instant> = None;
        let mut stream = response.bytes_stream();
        let outcome = loop {
            let next = tokio::select! {
                next = stream.next() => next,
                () = wait_for_cancel(cancel) => break Err(TranscribeError::Cancelled),
            };
            let chunk = match next {
                None => break Ok(()),
                Some(Ok(chunk)) => chunk,
                Some(Err(e)) => break Err(download_error(e)),
            };
            if downloaded + chunk.len() as u64 > file.size_bytes {
                drop(out);
                let _ = tokio::fs::remove_file(part).await;
                // Counted as no progress: the bytes were discarded.
                return Err((
                    0,
                    download_error(format!(
                        "server sent more than the expected {} bytes",
                        file.size_bytes
                    )),
                ));
            }
            if let Err(e) = out.write_all(&chunk).await {
                break Err(TranscribeError::Io(e));
            }
            downloaded += chunk.len() as u64;
            received += chunk.len() as u64;
            let now = Instant::now();
            let bytes_per_second = meter.record(now, downloaded);
            if last_report.is_none_or(|t| now.duration_since(t) >= PROGRESS_INTERVAL)
                || downloaded == file.size_bytes
            {
                last_report = Some(now);
                report(Phase::Downloading {
                    downloaded,
                    total: file.size_bytes,
                    bytes_per_second,
                });
            }
            if cancel.is_cancelled() {
                break Err(TranscribeError::Cancelled);
            }
        };
        // Keep what arrived, even when the transfer stopped early, so the next request resumes.
        let flushed = async {
            out.flush().await?;
            out.sync_all().await
        }
        .await;
        match (outcome, flushed) {
            (Err(error), _) => Err((received, error)),
            (Ok(()), Err(e)) => Err((received, TranscribeError::Io(e))),
            (Ok(()), Ok(())) if downloaded < file.size_bytes => Err((
                received,
                download_error("the connection closed before the file was complete"),
            )),
            (Ok(()), Ok(())) => Ok(received),
        }
    }
}

/// Completes when `cancel` is set (polled every [`CANCEL_POLL`]).
async fn wait_for_cancel(cancel: &CancelFlag) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// How the test server answers.
    #[derive(Debug, Clone, Default)]
    struct Behaviour {
        /// Answer every request with the whole file and 200, ignoring `Range`.
        ignore_range: bool,
        /// On the first request, close the connection after this many body bytes.
        cut_first_after: Option<usize>,
        /// Delay between 64 KiB body chunks.
        chunk_delay: Option<Duration>,
        /// Answer with this status and no body.
        status: Option<u16>,
    }

    /// A minimal HTTP/1.1 server on 127.0.0.1 serving one byte array, with optional `Range`.
    struct TestServer {
        url: String,
        /// (`Range` header, `User-Agent` header) of each request.
        requests: Arc<Log>,
    }

    impl TestServer {
        fn start(body: Vec<u8>, behaviour: Behaviour) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/model.bin", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let log = requests.clone();
            std::thread::spawn(move || {
                for (index, stream) in listener.incoming().enumerate() {
                    let Ok(stream) = stream else { break };
                    let body = body.clone();
                    let behaviour = behaviour.clone();
                    let log = log.clone();
                    std::thread::spawn(move || serve(stream, &body, &behaviour, index, &log));
                }
            });
            Self { url, requests }
        }

        fn ranges(&self) -> Vec<Option<String>> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.0.clone())
                .collect()
        }
    }

    /// (`Range` header, `User-Agent` header) of each request.
    type Log = Mutex<Vec<(Option<String>, Option<String>)>>;

    fn serve(stream: TcpStream, body: &[u8], b: &Behaviour, index: usize, log: &Log) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut range = None;
        let mut agent = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("range:") {
                range = Some(v.trim().to_owned());
            }
            if lower.starts_with("user-agent:") {
                agent = Some(line["user-agent:".len()..].trim().to_owned());
            }
        }
        log.lock().unwrap().push((range.clone(), agent));
        let mut out = stream;
        if let Some(status) = b.status {
            let _ = write!(
                out,
                "HTTP/1.1 {status} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
        let start: usize = match (&range, b.ignore_range) {
            (Some(r), false) => r
                .strip_prefix("bytes=")
                .and_then(|r| r.strip_suffix('-'))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            _ => 0,
        };
        if start > body.len() {
            let _ = write!(
                out,
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
        let slice = &body[start..];
        let head = if range.is_some() && !b.ignore_range {
            format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                slice.len(),
                start,
                body.len() - 1,
                body.len()
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                slice.len()
            )
        };
        if out.write_all(head.as_bytes()).is_err() {
            return;
        }
        let limit = match b.cut_first_after {
            Some(n) if index == 0 => n.min(slice.len()),
            _ => slice.len(),
        };
        for chunk in slice[..limit].chunks(64 * 1024) {
            if out.write_all(chunk).is_err() {
                return;
            }
            let _ = out.flush();
            if let Some(d) = b.chunk_delay {
                std::thread::sleep(d);
            }
        }
        let _ = out.shutdown(std::net::Shutdown::Both);
    }

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn pinned(body: &[u8], url: &str, name: &str) -> PinnedFile {
        PinnedFile {
            file_name: name.to_owned(),
            size_bytes: body.len() as u64,
            sha256: hex::encode(Sha256::digest(body)),
            url: url.to_owned(),
        }
    }

    fn info_for(file: &PinnedFile) -> ModelInfo {
        ModelInfo {
            model: SpeechModel::Fast,
            file_name: file.file_name.clone(),
            size_bytes: file.size_bytes,
            sha256: file.sha256.clone(),
            url: file.url.clone(),
        }
    }

    type Events = Arc<Mutex<Vec<ModelState>>>;

    fn recorder() -> (Events, impl Fn(ModelStatus) + Send + Sync) {
        let events: Events = Arc::default();
        let sink = events.clone();
        (events, move |s: ModelStatus| {
            sink.lock().unwrap().push(s.state)
        })
    }

    #[tokio::test]
    async fn downloads_verifies_and_reports_progress_in_order() {
        let data = body(300_000);
        let server = TestServer::start(data.clone(), Behaviour::default());
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path().join("models"));
        let (events, on_progress) = recorder();

        let path = store
            .download_files(&info_for(&file), &[], &on_progress, &CancelFlag::new())
            .await
            .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), data);
        assert!(!part_path(&path).exists());
        let events = events.lock().unwrap();
        assert!(matches!(events[0], ModelState::Downloading { .. }));
        let n = events.len();
        assert_eq!(events[n - 2], ModelState::Verifying);
        assert_eq!(events[n - 1], ModelState::Ready);
        assert!(events.iter().any(|e| matches!(
            e,
            ModelState::Downloading {
                downloaded: 300_000,
                total: 300_000,
                ..
            }
        )));
        assert_eq!(
            server.requests.lock().unwrap()[0].1.as_deref(),
            Some(USER_AGENT)
        );
        assert!(USER_AGENT.starts_with("MediaIdentifier/"));
    }

    #[tokio::test]
    async fn resumes_a_partial_file_with_a_range_request() {
        let data = body(200_000);
        let server = TestServer::start(data.clone(), Behaviour::default());
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        std::fs::write(dir.path().join("m.bin.part"), &data[..120_000]).unwrap();

        let path = store
            .download_files(&info_for(&file), &[], &|_| {}, &CancelFlag::new())
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(server.ranges(), vec![Some("bytes=120000-".to_owned())]);
    }

    #[tokio::test]
    async fn a_server_that_ignores_range_restarts_the_file() {
        let data = body(150_000);
        let server = TestServer::start(
            data.clone(),
            Behaviour {
                ignore_range: true,
                ..Behaviour::default()
            },
        );
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        std::fs::write(dir.path().join("m.bin.part"), &data[..50_000]).unwrap();

        let path = store
            .download_files(&info_for(&file), &[], &|_| {}, &CancelFlag::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
    }

    #[tokio::test]
    async fn a_dropped_connection_is_resumed_automatically() {
        let data = body(400_000);
        let server = TestServer::start(
            data.clone(),
            Behaviour {
                cut_first_after: Some(100_000),
                ..Behaviour::default()
            },
        );
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());

        let path = store
            .download_files(&info_for(&file), &[], &|_| {}, &CancelFlag::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        let ranges = server.ranges();
        assert_eq!(ranges.len(), 2, "{ranges:?}");
        assert_eq!(ranges[0], None);
        assert!(ranges[1].as_deref().unwrap().starts_with("bytes="));
    }

    #[tokio::test]
    async fn a_checksum_mismatch_deletes_the_partial_file() {
        let data = body(100_000);
        let server = TestServer::start(data.clone(), Behaviour::default());
        let mut file = pinned(&data, &server.url, "m.bin");
        file.sha256 = "0".repeat(64);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let (events, on_progress) = recorder();

        let err = store
            .download_files(&info_for(&file), &[], &on_progress, &CancelFlag::new())
            .await
            .unwrap_err();

        assert!(
            matches!(err, TranscribeError::ChecksumMismatch { .. }),
            "{err:?}"
        );
        assert!(!dir.path().join("m.bin").exists());
        assert!(!dir.path().join("m.bin.part").exists());
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(ModelState::Failed { .. })
        ));
    }

    #[tokio::test]
    async fn cancelling_keeps_the_partial_file_for_the_next_call() {
        let data = body(2_000_000);
        let server = TestServer::start(
            data.clone(),
            Behaviour {
                chunk_delay: Some(Duration::from_millis(20)),
                ..Behaviour::default()
            },
        );
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let cancel = CancelFlag::new();
        let trigger = cancel.clone();
        let (events, record) = recorder();
        let on_progress = move |s: ModelStatus| {
            if matches!(s.state, ModelState::Downloading { .. }) {
                trigger.cancel();
            }
            record(s);
        };

        let err = store
            .download_files(&info_for(&file), &[], &on_progress, &cancel)
            .await
            .unwrap_err();

        assert!(matches!(err, TranscribeError::Cancelled), "{err:?}");
        let kept = file_len(&dir.path().join("m.bin.part")).unwrap();
        assert!(kept > 0 && kept < data.len() as u64, "kept {kept}");
        assert!(
            !events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, ModelState::Failed { .. } | ModelState::Ready))
        );

        let path = store
            .download_files(&info_for(&file), &[], &|_| {}, &CancelFlag::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(
            server.ranges().last().unwrap().as_deref(),
            Some(format!("bytes={kept}-").as_str())
        );
    }

    #[tokio::test]
    async fn an_existing_final_file_is_not_downloaded_again() {
        let data = body(10_000);
        let server = TestServer::start(data.clone(), Behaviour::default());
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.bin"), &data).unwrap();
        let store = ModelStore::new(dir.path());
        let (events, on_progress) = recorder();

        store
            .download_files(&info_for(&file), &[], &on_progress, &CancelFlag::new())
            .await
            .unwrap();
        assert!(server.ranges().is_empty());
        assert_eq!(*events.lock().unwrap(), vec![ModelState::Ready]);
    }

    #[tokio::test]
    async fn an_oversized_partial_file_is_restarted() {
        let data = body(50_000);
        let server = TestServer::start(data.clone(), Behaviour::default());
        let file = pinned(&data, &server.url, "m.bin");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.bin.part"), body(60_000)).unwrap();
        let store = ModelStore::new(dir.path());

        let path = store
            .download_files(&info_for(&file), &[], &|_| {}, &CancelFlag::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(server.ranges(), vec![None]);
    }

    #[tokio::test]
    async fn http_errors_fail_after_three_attempts_without_a_final_file() {
        let server = TestServer::start(
            Vec::new(),
            Behaviour {
                status: Some(404),
                ..Behaviour::default()
            },
        );
        let file = PinnedFile {
            file_name: "m.bin".into(),
            size_bytes: 10,
            sha256: "0".repeat(64),
            url: server.url.clone(),
        };
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let (events, on_progress) = recorder();

        let err = store
            .download_files(&info_for(&file), &[], &on_progress, &CancelFlag::new())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, TranscribeError::Download(m) if m.contains("404")),
            "{err:?}"
        );
        assert_eq!(server.ranges().len(), 3);
        assert!(!dir.path().join("m.bin").exists());
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(ModelState::Failed { .. })
        ));
    }

    #[tokio::test]
    async fn extra_files_are_downloaded_before_ready() {
        let main = body(20_000);
        let vad = body(3_000);
        let main_server = TestServer::start(main.clone(), Behaviour::default());
        let vad_server = TestServer::start(vad.clone(), Behaviour::default());
        let main_file = pinned(&main, &main_server.url, "m.bin");
        let vad_file = pinned(&vad, &vad_server.url, "v.bin");
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let (events, on_progress) = recorder();

        store
            .download_files(
                &info_for(&main_file),
                std::slice::from_ref(&vad_file),
                &on_progress,
                &CancelFlag::new(),
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(dir.path().join("v.bin")).unwrap(), vad);
        assert_eq!(events.lock().unwrap().last(), Some(&ModelState::Ready));
    }

    #[test]
    fn status_reflects_the_files_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let model = SpeechModel::Fast;
        let info = model_info(model);
        assert_eq!(store.status(model).state, ModelState::Missing);
        assert_eq!(store.status(model).info, info);

        let part = part_path(&store.model_path(model));
        std::fs::write(&part, vec![0u8; 1234]).unwrap();
        assert_eq!(
            store.status(model).state,
            ModelState::Paused {
                downloaded: 1234,
                total: info.size_bytes
            }
        );

        // Sparse files of the pinned sizes stand in for finished downloads.
        std::fs::remove_file(&part).unwrap();
        std::fs::File::create(store.model_path(model))
            .unwrap()
            .set_len(info.size_bytes)
            .unwrap();
        assert_eq!(
            store.status(model).state,
            ModelState::Paused {
                downloaded: info.size_bytes,
                total: info.size_bytes
            },
            "the voice activity model is still missing"
        );
        assert_eq!(store.vad_model_path(), None);

        let vad = vad_model_file();
        std::fs::File::create(dir.path().join(&vad.file_name))
            .unwrap()
            .set_len(vad.size_bytes)
            .unwrap();
        assert_eq!(store.status(model).state, ModelState::Ready);
        assert_eq!(store.vad_model_path(), Some(dir.path().join(vad.file_name)));
        assert_eq!(
            store.status(SpeechModel::Accurate).state,
            ModelState::Missing
        );
    }

    #[test]
    fn a_final_file_of_the_wrong_size_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        std::fs::write(store.model_path(SpeechModel::Fast), b"short").unwrap();
        assert_eq!(store.status(SpeechModel::Fast).state, ModelState::Missing);
    }

    #[test]
    fn speed_is_averaged_over_recent_samples() {
        let mut meter = SpeedMeter::default();
        let t0 = Instant::now();
        assert_eq!(meter.record(t0, 0), 0.0);
        let speed = meter.record(t0 + Duration::from_secs(2), 2_000_000);
        assert!((speed - 1_000_000.0).abs() < 1.0);
        // Old samples fall out of the window.
        meter.record(t0 + Duration::from_secs(10), 2_000_000);
        let speed = meter.record(t0 + Duration::from_secs(11), 3_000_000);
        assert!((speed - 1_000_000.0).abs() < 1.0, "{speed}");
    }
}
