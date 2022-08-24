use std::{
    error::Error,
    future::Future,
    io::SeekFrom,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader},
    sync::mpsc::{
        error::{SendError, TryRecvError},
        Receiver, Sender,
    },
    time::sleep,
};

#[derive(Debug, Clone, Copy)]
pub enum LogReaderMode {
    ReadToEnd,
    NextLine,
}

#[derive(Debug)]
struct LogBufReader {
    file: BufReader<File>,
    sender: Arc<Sender<Vec<u8>>>,
    path: PathBuf,
    last_ctime: u64,
    mode: LogReaderMode,
}

pub struct LogWatcherBuilder {
    path: PathBuf,
    mode: LogReaderMode,
    skip_to_end: bool,
}

impl LogWatcherBuilder {
    pub fn mode(self, mode: LogReaderMode) -> Self {
        Self { mode, ..self }
    }

    pub fn skip_to_end(self, skip_to_end: bool) -> Self {
        Self {
            skip_to_end,
            ..self
        }
    }

    pub fn build(self) -> LogWatcher {
        let (sender, receiver) = tokio::sync::mpsc::channel(4096);
        let (signal_tx, signal_rx) = tokio::sync::mpsc::channel(4096);

        LogWatcher {
            receiver,
            sender: Arc::new(sender),
            path: self.path,
            signal_rx: Some(signal_rx).into(),
            signal_tx,
            mode: self.mode,
            skip_to_end: self.skip_to_end,
        }
    }
}

#[derive(Debug)]
pub struct LogWatcher {
    receiver: Receiver<Vec<u8>>,
    sender: Arc<Sender<Vec<u8>>>,
    path: PathBuf,
    signal_tx: Sender<LogWatcherSignal>,
    signal_rx: std::sync::Mutex<Option<Receiver<LogWatcherSignal>>>,
    mode: LogReaderMode,
    skip_to_end: bool,
}

#[derive(Debug)]
enum DetachedLogWatcher {
    Initializing(LogBufReader),
    Waiting(LogBufReader),
    Reading(LogBufReader),
    Missing(LogBufReader),
    Reloading((PathBuf, Arc<Sender<Vec<u8>>>, LogReaderMode)),
    Closed,
}
#[derive(Debug)]
pub enum LogWatcherSignal {
    Close,
    Reload,
    Swap(PathBuf),
}

type BoxedError = Box<dyn Error + 'static + Send + Sync>;
type BoxedResult<T> = Result<T, BoxedError>;
type SpawnFnResult = Pin<Box<dyn Future<Output = BoxedResult<()>> + Send + Sync>>;

impl LogWatcher {
    pub fn builder(file_path: impl Into<PathBuf>) -> LogWatcherBuilder {
        LogWatcherBuilder {
            path: file_path.into(),
            mode: LogReaderMode::ReadToEnd,
            skip_to_end: true,
        }
    }

    pub fn set_mode(self, mode: LogReaderMode) -> Self {
        Self { mode, ..self }
    }

    pub async fn send_signal(
        &self,
        signal: LogWatcherSignal,
    ) -> Result<(), SendError<LogWatcherSignal>> {
        self.signal_tx.send(signal).await
    }

    pub async fn read_message(&mut self) -> Option<Vec<u8>> {
        self.receiver.recv().await
    }

    pub fn try_read_message(&mut self) -> Result<Vec<u8>, TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn spawn(&self) -> SpawnFnResult {
        let sender = self.sender.clone();
        let path = self.path.clone();

        let signal_rx = self.signal_rx.lock().unwrap().take();

        if signal_rx.is_none() {
            panic!("Log watcher spanwed twice {:?}", self);
        };

        let mut signal_rx = signal_rx.unwrap();

        let mode = self.mode.clone();
        let skip_to_end = self.skip_to_end;

        let future: SpawnFnResult = Box::pin(async move {
            let mut detached = match File::open(&path).await {
                Ok(file) => {
                    if skip_to_end {
                        DetachedLogWatcher::Initializing(LogBufReader {
                            file: BufReader::new(file),
                            sender: sender.clone(),
                            path: path.clone(),
                            last_ctime: get_c_time(&path).await.unwrap(),
                            mode,
                        })
                    } else {
                        DetachedLogWatcher::Waiting(LogBufReader {
                            file: BufReader::new(file),
                            sender: sender.clone(),
                            path: path.clone(),
                            last_ctime: get_c_time(&path).await.unwrap(),
                            mode,
                        })
                    }
                }
                Err(err) => match err.kind() {
                    std::io::ErrorKind::NotFound => {
                        DetachedLogWatcher::Reloading((path.clone(), sender.clone(), mode))
                    }
                    _ => Err(err)?,
                },
            };

            loop {
                match signal_rx.try_recv() {
                    Ok(LogWatcherSignal::Close) => {
                        detached.close().await;
                    }
                    Ok(LogWatcherSignal::Reload) => {
                        detached.reload().await;
                    }
                    Ok(LogWatcherSignal::Swap(path)) => {
                        detached.swap(path).await;
                    }
                    Err(err) => {
                        if err == TryRecvError::Disconnected {
                            break;
                        }
                    }
                }

                match detached {
                    DetachedLogWatcher::Closed => {
                        break;
                    }
                    _ => {
                        detached =
                            match detached.next().await {
                                Ok(next) => next,
                                Err(err) => match err.kind() {
                                    std::io::ErrorKind::NotFound => DetachedLogWatcher::Reloading(
                                        (path.clone(), sender.clone(), mode),
                                    ),
                                    _ => Err(err)?,
                                },
                            };
                    }
                }
            }

            Ok(())
        });
        future
    }
}

impl DetachedLogWatcher {
    pub async fn next(self) -> Result<Self, std::io::Error> {
        match self {
            DetachedLogWatcher::Initializing(mut inner) => {
                inner.skip_file().await?;
                Ok(DetachedLogWatcher::Waiting(inner))
            }
            DetachedLogWatcher::Waiting(mut inner) => match inner.read_next().await {
                Ok(size) if size > 4096 => Ok(DetachedLogWatcher::Reading(inner)),
                Ok(size) => {
                    if size == 0 {
                        let curr_ctime = get_c_time(&inner.path).await?;

                        if curr_ctime > inner.last_ctime {
                            return Ok(DetachedLogWatcher::Missing(inner));
                        }

                        sleep(Duration::from_millis(200)).await;
                        Ok(DetachedLogWatcher::Waiting(inner))
                    } else {
                        sleep(Duration::from_millis(200)).await;
                        Ok(DetachedLogWatcher::Waiting(inner))
                    }
                }
                Err(err) => match err.kind() {
                    std::io::ErrorKind::NotFound => Ok(DetachedLogWatcher::Missing(inner)),
                    _ => Err(err),
                },
            },
            DetachedLogWatcher::Reading(mut inner) => match inner.read_next().await {
                Ok(size) if size < 4096 => Ok(DetachedLogWatcher::Waiting(inner)),
                Ok(_) => Ok(DetachedLogWatcher::Reading(inner)),
                Err(err) => match err.kind() {
                    std::io::ErrorKind::NotFound => Ok(DetachedLogWatcher::Missing(inner)),
                    _ => Err(err),
                },
            },
            DetachedLogWatcher::Missing(inner) => {
                inner.sender.try_send(inner.file.buffer().to_vec()).ok();
                Ok(DetachedLogWatcher::Reloading((
                    inner.path,
                    inner.sender,
                    inner.mode,
                )))
            }
            DetachedLogWatcher::Reloading((path, sender, mode)) => {
                let file_exists = match tokio::fs::metadata(&path).await {
                    Ok(meta) => Ok(meta.is_file()),
                    Err(err) => match err.kind() {
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
                            Ok(false)
                        }
                        _ => Err(err),
                    },
                }?;

                if file_exists {
                    let new_inner = LogBufReader {
                        file: BufReader::new(File::open(&path).await?),
                        path: path.clone(),
                        sender,
                        last_ctime: get_c_time(&path).await.unwrap(),
                        mode,
                    };

                    Ok(DetachedLogWatcher::Waiting(new_inner))
                } else {
                    sleep(Duration::from_secs(1)).await;
                    Ok(DetachedLogWatcher::Reloading((path, sender, mode)))
                }
            }
            DetachedLogWatcher::Closed => Ok(DetachedLogWatcher::Closed),
        }
    }

    pub async fn close(&mut self) {
        match self {
            DetachedLogWatcher::Initializing(inner)
            | DetachedLogWatcher::Waiting(inner)
            | DetachedLogWatcher::Reading(inner)
            | DetachedLogWatcher::Missing(inner) => {
                inner.read_next().await.ok();
                *self = DetachedLogWatcher::Closed
            }
            DetachedLogWatcher::Reloading(_) => *self = DetachedLogWatcher::Closed,
            DetachedLogWatcher::Closed => {}
        }
    }

    pub async fn reload(&mut self) {
        match self {
            DetachedLogWatcher::Initializing(inner)
            | DetachedLogWatcher::Waiting(inner)
            | DetachedLogWatcher::Reading(inner)
            | DetachedLogWatcher::Missing(inner) => {
                let result = inner.read_next().await.unwrap_or(0);

                if result == 0 {
                    inner.sender.try_send(inner.file.buffer().to_vec()).ok();
                }
                *self = DetachedLogWatcher::Reloading((
                    inner.path.clone(),
                    inner.sender.clone(),
                    inner.mode,
                ));
            }
            DetachedLogWatcher::Reloading(_) | DetachedLogWatcher::Closed => {}
        }
    }

    pub async fn swap(&mut self, path: PathBuf) {
        match self {
            DetachedLogWatcher::Initializing(inner)
            | DetachedLogWatcher::Waiting(inner)
            | DetachedLogWatcher::Reading(inner)
            | DetachedLogWatcher::Missing(inner) => {
                let result = inner.read_next().await.unwrap_or(0);

                if result == 0 {
                    inner.sender.try_send(inner.file.buffer().to_vec()).ok();
                }
                *self = DetachedLogWatcher::Reloading((path, inner.sender.clone(), inner.mode));
            }
            DetachedLogWatcher::Reloading((_old_path, sender, mode)) => {
                *self = DetachedLogWatcher::Reloading((path, sender.clone(), mode.clone()));
            }
            DetachedLogWatcher::Closed => {}
        }
    }
}

impl LogBufReader {
    async fn read_next(&mut self) -> Result<usize, std::io::Error> {
        match self.mode {
            LogReaderMode::ReadToEnd => self.read_to_end().await,
            LogReaderMode::NextLine => self.read_next_line().await,
        }
    }

    async fn read_next_line(&mut self) -> Result<usize, std::io::Error> {
        let mut buffer = String::new();
        const MAX_SIZE: usize = 4096 * 16;

        loop {
            let total_size = buffer.len();

            match self.file.read_line(&mut buffer).await {
                Ok(size) if size > 0 => {
                    if total_size > MAX_SIZE {
                        return match self.sender.try_send(buffer.into_bytes()) {
                            Ok(_) => match get_c_time(&self.path).await {
                                Ok(ctime) => {
                                    self.last_ctime = ctime;
                                    Ok(total_size)
                                }
                                Err(err) => match err.kind() {
                                    std::io::ErrorKind::NotFound => Ok(total_size),
                                    std::io::ErrorKind::UnexpectedEof => Ok(total_size),
                                    _ => Err(err),
                                },
                            },
                            Err(_) => Err(std::io::Error::new(
                                std::io::ErrorKind::NotConnected,
                                "failed to send to channel",
                            )),
                        };
                    } else {
                        continue;
                    };
                }
                Ok(_) => {
                    if buffer.len() == 0 {
                        return Ok(0);
                    }
                    return match self.sender.try_send(buffer.into_bytes()) {
                        Ok(_) => match get_c_time(&self.path).await {
                            Ok(ctime) => {
                                self.last_ctime = ctime;
                                Ok(total_size)
                            }
                            Err(err) => match err.kind() {
                                std::io::ErrorKind::NotFound => Ok(total_size),
                                std::io::ErrorKind::UnexpectedEof => Ok(total_size),
                                _ => Err(err),
                            },
                        },
                        Err(_) => Err(std::io::Error::new(
                            std::io::ErrorKind::NotConnected,
                            "failed to send to channel",
                        )),
                    };
                }
                Err(err) => {
                    return match err.kind() {
                        std::io::ErrorKind::UnexpectedEof => Ok(total_size),
                        std::io::ErrorKind::NotFound => Ok(total_size),
                        _ => Err(err),
                    }
                }
            }
        }
    }

    async fn read_to_end(&mut self) -> Result<usize, std::io::Error> {
        let mut buffer: Vec<u8> = Vec::new();
        let result: Result<usize, std::io::Error> = self.file.read_to_end(&mut buffer).await;
        match result {
            Ok(size) if size > 0 => match self.sender.try_send(buffer) {
                Ok(_) => match get_c_time(&self.path).await {
                    Ok(ctime) => {
                        self.last_ctime = ctime;
                        Ok(size)
                    }
                    Err(err) => match err.kind() {
                        std::io::ErrorKind::NotFound => Ok(0),
                        std::io::ErrorKind::UnexpectedEof => Ok(0),
                        _ => Err(err),
                    },
                },
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "failed to send to channel",
                )),
            },
            Ok(size) => Ok(size),
            Err(err) => match err.kind() {
                std::io::ErrorKind::UnexpectedEof => Ok(0),
                std::io::ErrorKind::NotFound => Ok(0),
                _ => Err(err),
            },
        }
    }

    async fn skip_file(&mut self) -> Result<(), std::io::Error> {
        self.file.seek(SeekFrom::End(0)).await?;
        Ok(())
    }
}

#[cfg(windows)]
async fn get_c_time(path: &Path) -> Result<u64, std::io::Error> {
    use std::os::windows::prelude::MetadataExt;

    let meta = tokio::fs::metadata(path).await?;
    Ok(meta.last_write_time())
}

#[cfg(unix)]
async fn get_c_time(path: &Path) -> Result<u64, std::io::Error> {
    use std::os::unix::prelude::MetadataExt;

    let meta = tokio::fs::metadata(path).await?;
    Ok(meta.ctime() as u64)
}
