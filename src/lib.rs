use std::{error::Error, io::SeekFrom, sync::Arc, time::Duration};

use async_fs::File;
use async_std::path::PathBuf;
use async_std::{
    channel::{Receiver, Sender},
    io::prelude::SeekExt,
    io::ReadExt,
};
use async_std::{io::BufReader, path::Path, task::sleep};

mod pretty_state;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(linux)]
use std::os::linux::fs::MetadataExt;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[derive(Debug)]
struct LogBufReader {
    file: BufReader<File>,
    sender: Arc<Sender<Vec<u8>>>,
    path: async_std::path::PathBuf,
    last_ctime: u64,
}

#[derive(Debug)]
pub struct LogWatcher {
    receiver: Receiver<Vec<u8>>,
    sender: Arc<Sender<Vec<u8>>>,
    path: PathBuf,
}

#[derive(Debug)]
enum DetachedLogWatcher {
    Initializing(LogBufReader),
    Waiting(LogBufReader),
    Reading(LogBufReader),
    Missing(LogBufReader),
    Reloading((async_std::path::PathBuf, Arc<Sender<Vec<u8>>>)),
    Closed,
}

#[allow(dead_code)]
pub enum LogWatcherSignal {
    Close,
    Reload,
    Swap(PathBuf),
}

impl LogWatcher {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        let (sender, receiver) = async_std::channel::unbounded();

        Self {
            receiver,
            sender: Arc::new(sender),
            path: file_path.into(),
        }
    }

    pub async fn spawn(
        &self,
        skip_to_end: bool,
    ) -> Result<Sender<LogWatcherSignal>, Box<dyn Error>> {
        let sender = self.sender.clone();
        let file = File::open(&self.path).await?;
        let (signal_tx, signal_rx) = async_std::channel::unbounded();
        let path = self.path.clone();

        async_std::task::spawn(async move {
            let mut detached = if skip_to_end {
                DetachedLogWatcher::Initializing(LogBufReader {
                    file: BufReader::new(file),
                    sender,
                    path: path.clone().into(),
                    last_ctime: get_c_time(&path).await.unwrap(),
                })
            } else {
                DetachedLogWatcher::Reading(LogBufReader {
                    file: BufReader::new(file),
                    sender,
                    path: path.clone().into(),
                    last_ctime: get_c_time(&path).await.unwrap(),
                })
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
                    Err(err) => match err {
                        async_std::channel::TryRecvError::Closed => {
                            break;
                        }
                        _ => {}
                    },
                }

                match detached {
                    DetachedLogWatcher::Closed => {
                        break;
                    }
                    _ => {
                        detached = detached
                            .next()
                            .await
                            .expect("failed to move next on detached log watcher");
                    }
                }
            }
        });

        Ok(signal_tx)
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

                        sleep(Duration::from_secs(1)).await;
                        Ok(DetachedLogWatcher::Waiting(inner))
                    } else {
                        sleep(Duration::from_secs(1)).await;
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
                Ok(DetachedLogWatcher::Reloading((inner.path, inner.sender)))
            }
            DetachedLogWatcher::Reloading((path, sender)) => {
                if path.exists().await {
                    let new_inner = LogBufReader {
                        file: BufReader::new(File::open(&path).await?),
                        path: path.clone(),
                        sender,
                        last_ctime: get_c_time(&path).await.unwrap(),
                    };

                    Ok(DetachedLogWatcher::Waiting(new_inner))
                } else {
                    sleep(Duration::from_secs(1)).await;
                    Ok(DetachedLogWatcher::Reloading((path, sender)))
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
                *self = DetachedLogWatcher::Reloading((inner.path.clone(), inner.sender.clone()));
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
                *self = DetachedLogWatcher::Reloading((path, inner.sender.clone()));
            }
            DetachedLogWatcher::Reloading((_old_path, sender)) => {
                *self = DetachedLogWatcher::Reloading((path, sender.clone()));
            }
            DetachedLogWatcher::Closed => {}
        }
    }
}

impl LogBufReader {
    async fn read_next(&mut self) -> Result<usize, std::io::Error> {
        let mut buffer: Vec<u8> = Vec::new();
        let result: Result<usize, std::io::Error> = self.file.read_to_end(&mut buffer).await;

        match result {
            Ok(size) if size > 0 => match self.sender.try_send(buffer) {
                Ok(_) => {
                    self.last_ctime = get_c_time(&self.path).await?;
                    Ok(size)
                }
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "failed to send to channel",
                )),
            },
            Ok(size) => Ok(size),
            Err(err) => match err.kind() {
                std::io::ErrorKind::UnexpectedEof => Ok(0),
                _ => Err(err),
            },
        }
    }

    async fn skip_file(&mut self) -> Result<(), std::io::Error> {
        self.file.seek(SeekFrom::End(0)).await?;
        Ok(())
    }
}

impl std::ops::Deref for LogWatcher {
    type Target = Receiver<Vec<u8>>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

#[cfg(windows)]
async fn get_c_time(path: &Path) -> Result<u64, std::io::Error> {
    Ok(async_fs::metadata(path).await?.last_write_time())
}

#[cfg(unix)]
async fn get_c_time(path: &Path) -> Result<u64, std::io::Error> {
    Ok(async_fs::metadata(path).await?.ctime() as u64)
}
