#[cfg(unix)]
use std::{os::unix::fs::MetadataExt, todo};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use std::io::SeekFrom;

use async_fs::File;
use async_std::{
    io::{prelude::BufReadExt, prelude::SeekExt, BufReader, ReadExt},
    path::{Path, PathBuf},
};

enum FileWatcher {
    Initilizing(FileWatcherState<Initilizing>),
    Reading(FileWatcherState<Reading>),
    Waiting(FileWatcherState<Waiting>),
}

impl FileWatcher {
    pub async fn next(mut self) -> Result<Self, std::io::Error> {
        match self {
            FileWatcher::Initilizing(file_watcher) => {
                Ok(FileWatcher::Reading(file_watcher.next().await.unwrap()))
            }
            FileWatcher::Reading(_) => todo!(),
            FileWatcher::Waiting(_) => todo!(),
        }
    }
}

struct Initilizing {
    file: BufReader<File>,
}

struct Reading {
    file: BufReader<File>,
    last_ctime: u64,
}

struct Waiting {
    file: BufReader<File>,
    last_ctime: u64,
}

struct FileWatcherState<T> {
    state: T,
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
                file: BufReader::new(file),
            },
        }))
    }

    async fn next(mut self) -> Result<FileWatcherState<Reading>, std::io::Error> {
        self.state.file.seek(SeekFrom::End(0)).await?;
        let last_ctime = get_c_time(&self.path).await?;

        Ok(FileWatcherState {
            output_rx: self.output_rx,
            output_tx: self.output_tx,
            path: self.path,
            state: Reading {
                file: self.state.file,
                last_ctime,
            },
        })
    }
}

impl FileWatcherState<Reading> {
    pub async fn read(&mut self) -> Result<usize, std::io::Error> {
        let mut buffer: Vec<u8> = Vec::new();
        self.state.file.read_to_end(&mut buffer).await
    }

    pub async fn wait(mut self) -> Result<FileWatcherState<Waiting>, std::io::Error> {
        let last_ctime = get_c_time(&self.path).await?;

        Ok(FileWatcherState {
            output_rx: self.output_rx,
            output_tx: self.output_tx,
            path: self.path,
            state: Waiting {
                file: self.state.file,
                last_ctime,
            },
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
        file_watcher = file_watcher.next().await.unwrap();
    }
}
