mod scenario;
#[cfg(test)]
mod tests {
    use async_log_watcher::LogWatcherSignal;
    use async_std::task::sleep;
    use std::time::{Duration, Instant};

    use crate::scenario::TestWriter;

    #[async_std::test]
    async fn it_works() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let mut test_writer = TestWriter::new("test_data", "test.txt", 1).await;

        let log_watcher = async_log_watcher::LogWatcher::new("test_data/test.txt");
        let log_watcher_channel = log_watcher.spawn().await.unwrap();

        test_writer.start().await;
        let now = Instant::now();
        while now.elapsed().as_millis() < 1000 {
            if let Ok(data) = test_writer.written_rx.try_recv() {
                written.push(data);
            }
            if let Ok(data) = log_watcher.try_recv() {
                for line in std::str::from_utf8(&data).unwrap().split("\n") {
                    if line.len() > 0 {
                        read.push(format!("{}\n", line));
                    }
                }
            }
        }

        test_writer.stop();
        sleep(Duration::from_secs(1)).await;
        log_watcher_channel.send(LogWatcherSignal::Close).await;

        if let Ok(data) = test_writer.written_rx.try_recv() {
            written.push(data);
        }
        if let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    read.push(format!("{}\n", line));
                }
            }
        }

        sleep(Duration::from_secs(1)).await;

        for idx in 0..read.len() {
            print!("{}", read[idx]);
            assert_eq!(read[idx], written[idx]);
        }
    }

    #[async_std::test]
    async fn reload_working() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let mut test_writer = TestWriter::new("test_data", "test.txt", 1).await;

        let log_watcher = async_log_watcher::LogWatcher::new("test_data/test.txt");
        let log_watcher_channel = log_watcher.spawn().await.unwrap();

        test_writer.start().await;
        let now = Instant::now();

        while now.elapsed().as_millis() < 1000 {
            if let Ok(data) = test_writer.written_rx.try_recv() {
                written.push(data);
            }
            if let Ok(data) = log_watcher.try_recv() {
                for line in std::str::from_utf8(&data).unwrap().split("\n") {
                    if line.len() > 0 {
                        read.push(format!("{}\n", line));
                    }
                }
            }
        }

        println!("reloading");
        test_writer.close_and_make_new().await;

        let now = Instant::now();
        while now.elapsed().as_millis() < 1000 {
            if let Ok(data) = test_writer.written_rx.try_recv() {
                written.push(data);
            }
            if let Ok(data) = log_watcher.try_recv() {
                for line in std::str::from_utf8(&data).unwrap().split("\n") {
                    if line.len() > 0 {
                        read.push(format!("{}\n", line));
                    }
                }
            }
        }

        sleep(Duration::from_secs(1)).await;
        log_watcher_channel.send(LogWatcherSignal::Close).await;

        if let Ok(data) = test_writer.written_rx.try_recv() {
            written.push(data);
        }
        if let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    read.push(format!("{}\n", line));
                }
            }
        }

        sleep(Duration::from_secs(1)).await;

        for idx in 0..read.len() {
            print!("{}", read[idx]);
            assert_eq!(read[idx], written[idx]);
        }
    }
}
