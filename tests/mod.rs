#[cfg(test)]
mod tests {
    use async_log_watcher::LogWatcherSignal;
    use async_std::{
        sync::{Receiver, Sender},
        task::sleep,
    };
    use futures_lite::AsyncWriteExt;
    use std::time::{Duration, Instant};

    #[async_std::test]
    async fn it_works() {
        let (cancel_sender, cancel) = async_std::sync::channel::<()>(1);
        let (start_sender, start) = async_std::sync::channel::<()>(1);

        let (control, verification) = async_std::sync::channel::<String>(1024);
        spawn_log_writer(1, start, cancel, control).await;

        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];

        let log_watcher = async_log_watcher::LogWatcher::new("test_data/test.txt");
        let channel = log_watcher.spawn().await.unwrap();
        let now = Instant::now();
        start_sender.send(()).await;

        while now.elapsed().as_millis() < 1000 {
            if let Ok(data) = verification.try_recv() {
                written.push(data);
            }
            if let Ok(data) = log_watcher.try_recv() {
                for line in std::str::from_utf8(&data).unwrap().split("\n") {
                    if line.len() > 0 {
                        read.push(format!("{}\n", line));
                        println!("{}", line);
                    }
                }
            }
        }
        cancel_sender.send(()).await;
        sleep(Duration::from_secs(1)).await;
        channel.send(LogWatcherSignal::Close).await;

        if let Ok(data) = verification.try_recv() {
            written.push(data);
        }
        if let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    read.push(format!("{}\n", line));
                    println!("{}", line);
                }
            }
        }

        sleep(Duration::from_secs(1)).await;

        for idx in 0..read.len() {
            assert_eq!(read[idx], written[idx]);
        }
    }

    async fn spawn_log_writer(
        sleep_millis: u64,
        start: Receiver<()>,
        cancel: Receiver<()>,
        written: Sender<String>,
    ) {
        async_std::task::spawn(async move {
            async_fs::create_dir("test_data").await.ok();
            let mut writer = async_std::io::BufWriter::new(
                async_fs::File::create("test_data/test.txt").await.unwrap(),
            );

            while start.is_empty() {
                sleep(Duration::from_millis(1)).await;
            }

            let mut x = 1;
            while cancel.is_empty() {
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
                let result_line = format!("{}-{}", x, rand_line);
                writer.write(result_line.as_bytes()).await.unwrap();
                written.send(result_line).await;
                writer.flush().await.unwrap();
                x = x + 1;
                sleep(Duration::from_millis(sleep_millis)).await;
            }
        });
    }
}
