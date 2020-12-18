#[cfg(unix)]
use std::{os::unix::fs::MetadataExt, todo};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use std::io::SeekFrom;

use async_fs::File;
use async_std::{
    io::{prelude::SeekExt, BufReader},
    path::{Path, PathBuf},
};

enum FileWatcherState {
    Initilizing(FileWatcher<Initilizing>),
    Reading(FileWatcher<Reading>),
}

impl FileWatcherState {
    pub async fn next(mut self) -> Result<Self, std::io::Error> {
        match self {
            FileWatcherState::Initilizing(file_watcher) => Ok(FileWatcherState::Reading(
                file_watcher.next().await.unwrap(),
            )),
            FileWatcherState::Reading(_) => todo!(),
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

struct FileWatcher<T> {
    state: T,
    path: PathBuf,
    output_rx: async_std::channel::Receiver<Vec<u8>>,
    output_tx: async_std::channel::Sender<Vec<u8>>,
}

impl FileWatcher<Initilizing> {
    async fn new(path: impl AsRef<Path>) -> Result<FileWatcherState, std::io::Error>
where {
        let file = File::open(path.as_ref()).await?;
        let (output_tx, output_rx) = async_std::channel::unbounded();
        Ok(FileWatcherState::Initilizing(FileWatcher {
            path: path.as_ref().clone().into(),
            output_rx,
            output_tx,
            state: Initilizing {
                file: BufReader::new(file),
            },
        }))
    }

    async fn next(mut self) -> Result<FileWatcher<Reading>, std::io::Error> {
        self.state.file.seek(SeekFrom::End(0)).await?;
        let last_ctime = get_c_time(&self.path).await?;

        Ok(FileWatcher {
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
    use super::{FileWatcher, Initilizing};

    #[async_std::test]
    async fn init() {
        let mut file_watcher = FileWatcher::new("test_data/test.txt").await.unwrap();
        file_watcher = file_watcher.next().await.unwrap();
    }
}
