use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use async_fs::File;
use async_std::{
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{Receiver, Sender},
    task::sleep,
};
use futures_lite::AsyncWriteExt;

pub struct TestWriter {
    sleep_millis: u64,
    written_tx: Option<Sender<String>>,
    pub written_rx: Receiver<String>,
    pub file_path: PathBuf,
    writer: Option<BufWriter<File>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl TestWriter {
    pub async fn new<T: AsRef<Path>>(path: T, file_name: T, sleep_millis: u64) -> Self {
        let path = path.as_ref();
        async_fs::create_dir(path).await.ok();

        let writer = async_std::io::BufWriter::new(
            async_fs::File::create(path.join(file_name.as_ref()))
                .await
                .unwrap(),
        );

        let (written_tx, written_rx) = async_std::sync::channel::<String>(4096);

        Self {
            file_path: path.join(file_name).to_path_buf(),
            sleep_millis,
            written_rx,
            written_tx: Some(written_tx),
            writer: Some(writer),
            cancel: Arc::new(false.into()),
        }
    }

    pub async fn start(&mut self) {
        self.cancel.swap(false, Ordering::SeqCst);
        let cancel = self.cancel.clone();
        let sleep_millis = self.sleep_millis;
        let written_tx = self.written_tx.take().unwrap();

        let mut writer = self.writer.take().unwrap();

        async_std::task::spawn(async move {
            let mut count = 0;
            while cancel.load(Ordering::SeqCst) == false {
                sleep(Duration::from_millis(sleep_millis)).await;
                let data = gen_random_string(count);
                writer.write_all(data.as_bytes()).await.unwrap();
                written_tx.send(data).await;
                count = count + 1;
                writer.flush().await.unwrap();
            }

            writer.flush().await.unwrap();
            writer.into_inner().await.unwrap().close().await.unwrap();
        });
    }

    pub fn stop(&mut self) {
        self.cancel.swap(true, Ordering::SeqCst);
    }

    pub async fn close_and_make_new(&mut self) {
        self.stop();
        let (written_tx, written_rx) = async_std::sync::channel::<String>(4096);

        self.written_rx = written_rx;
        self.written_tx = Some(written_tx);
        sleep(Duration::from_millis(self.sleep_millis * 10)).await;
        let writer = async_std::io::BufWriter::new(
            async_fs::File::create(self.file_path.as_path())
                .await
                .unwrap(),
        );
        self.writer = Some(writer);
        self.start().await;
    }
}

fn gen_random_string(idx: u64) -> String {
    let mut rand_line: String = (1..99)
        .map(|_| {
            let rand_char = (rand::random::<u8>() % 0x5E) + 0x20;
            std::str::from_utf8(&vec![rand_char])
                .unwrap()
                .to_owned()
                .pop()
                .unwrap()
        })
        .collect();
    rand_line.push('\n');
    format!("{}-{}", idx, rand_line)
}
