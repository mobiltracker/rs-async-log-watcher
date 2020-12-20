#[cfg(unix)]
use std::{os::unix::fs::MetadataExt, time::Duration, todo};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use std::io::SeekFrom;

use async_fs::File;
use async_std::{
    io::{prelude::BufReadExt, prelude::SeekExt, BufReader, ReadExt},
    path::{Path, PathBuf},
    task::sleep,
};
use futures_lite::AsyncBufReadExt;

static MAX_BUFFER_SIZE: usize = 1024 * 16; //16KB

#[derive(Debug)]
pub enum FileWatcher {
    Initilizing(FileWatcherState<Initilizing>),
    Reading(FileWatcherState<Reading>),
}

impl FileWatcher {
    pub async fn next(self) -> Result<Self, std::io::Error> {
        match self {
            FileWatcher::Initilizing(state) => Ok(FileWatcher::Reading(state.to_reading().await?)),
            FileWatcher::Reading(mut state) => match state.state.last_bytes_read {
                Some(bytes) => {
                    let mut new = Vec::with_capacity(bytes);
                    std::mem::swap(&mut new, &mut state.buffer);
                    state.output_tx.send(new).await.unwrap();

                    if bytes < 4096 {
                        sleep(Duration::from_secs(1)).await;
                    }
                    Ok(FileWatcher::Reading(state.read().await?))
                }
                None => Ok(FileWatcher::Reading(state.read().await?)),
            },
        }
    }

    pub async fn new(path: impl AsRef<Path>) -> Self {
        FileWatcherState::new(path).await.unwrap()
    }

    pub fn get_channel(&self) -> async_std::channel::Receiver<Vec<u8>> {
        match self {
            FileWatcher::Initilizing(state) => state.output_rx.clone(),
            FileWatcher::Reading(state) => state.output_rx.clone(),
        }
    }
}
#[derive(Debug)]
pub struct Initilizing {
    file: BufReader<File>,
}
#[derive(Debug)]
pub struct Reading {
    file: BufReader<File>,
    last_ctime: u64,
    last_bytes_read: Option<usize>,
}
#[derive(Debug)]
pub struct FileWatcherState<T> {
    state: T,
    buffer: Vec<u8>,
    path: PathBuf,
    output_rx: async_std::channel::Receiver<Vec<u8>>,
    output_tx: async_std::channel::Sender<Vec<u8>>,
}

impl FileWatcherState<Initilizing> {
    async fn new(path: impl AsRef<Path>) -> Result<FileWatcher, std::io::Error>
where {
        let file = File::open(path.as_ref()).await?;
        let (output_tx, output_rx) = async_std::channel::unbounded();
        Ok(FileWatcher::Initilizing(FileWatcherState {
            path: path.as_ref().clone().into(),
            output_rx,
            output_tx,
            state: Initilizing {
                file: BufReader::with_capacity(4096, file),
            },
            buffer: Vec::with_capacity(4096),
        }))
    }

    async fn to_reading(mut self) -> Result<FileWatcherState<Reading>, std::io::Error> {
        self.state.file.seek(SeekFrom::End(0)).await?;
        let last_ctime = get_c_time(&self.path).await?;

        Ok(FileWatcherState {
            output_rx: self.output_rx,
            output_tx: self.output_tx,
            path: self.path,
            buffer: self.buffer,
            state: Reading {
                file: self.state.file,
                last_ctime,
                last_bytes_read: None,
            },
        })
    }
}

impl FileWatcherState<Reading> {
    pub async fn read(mut self) -> Result<FileWatcherState<Reading>, std::io::Error> {
        let last_ctime = get_c_time(&self.path).await?;
        let bytes_read = self.state.file.read_to_end(&mut self.buffer).await?;

        Ok(FileWatcherState {
            state: Reading {
                file: self.state.file,
                last_ctime,
                last_bytes_read: Some(bytes_read),
            },
            buffer: self.buffer,
            path: self.path,
            output_rx: self.output_rx,
            output_tx: self.output_tx,
        })
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

#[cfg(test)]
mod test {
    use super::{FileWatcherState, Initilizing};

    #[async_std::test]
    async fn init() {
        let mut file_watcher = FileWatcherState::new("test_data/test.txt").await.unwrap();
        file_watcher = file_watcher
            .next()
            .await
            .unwrap()
            .next()
            .await
            .unwrap()
            .next()
            .await
            .unwrap();
    }
}
