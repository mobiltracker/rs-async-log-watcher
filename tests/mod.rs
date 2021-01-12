mod scenario;
#[cfg(test)]
mod tests {
    use crate::scenario::TestWriter;
    use async_log_watcher::{pretty_state::FileWatcher, LogWatcherSignal};
    use async_std::task::sleep;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    #[async_std::test]
    async fn it_works() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let count = 500;
        let mut test_writer = TestWriter::new("test_data", "test_single.txt", 1, count).await;
        let log_watcher = async_log_watcher::LogWatcher::new("test_data/test_single.txt");
        let log_watcher_channel = log_watcher.spawn(false).await.unwrap();

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            if let Ok(data) = test_writer.written_rx.try_recv() {
                written.push(data);
            }
        }

        sleep(Duration::from_secs(1)).await;

        log_watcher_channel
            .send(LogWatcherSignal::Close)
            .await
            .unwrap();

        while let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    println!("{:?}", line);
                    read.push(format!("{}\n", line));
                }
            }
        }

        while let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    read.push(format!("{}\n", line));
                }
            }
        }

        assert_eq!(read.len(), count as usize);
        for idx in 0..read.len() {
            assert_eq!(read[idx], written[idx]);
        }
    }

    #[async_std::test]
    async fn reload_working() {
        let mut written_first_round: Vec<String> = vec![];
        let mut written_second_round: Vec<String> = vec![];

        let mut read_first_round: Vec<String> = vec![];
        let mut read_second_round: Vec<String> = vec![];
        let count = 500;
        let mut test_writer = TestWriter::new("test_data", "test_reload.txt", 1, count).await;

        let log_watcher = async_log_watcher::LogWatcher::new("test_data/test_reload.txt");
        let log_watcher_channel = log_watcher.spawn(false).await.unwrap();

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            while let Ok(data) = test_writer.written_rx.try_recv() {
                written_first_round.push(data);
            }
        }

        sleep(Duration::from_millis(1000)).await;

        while let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    read_first_round.push(format!("{}\n", line));
                }
            }
        }

        let mut test_writer = TestWriter::new("test_data", "test_reload.txt", 1, count).await;
        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            while let Ok(data) = test_writer.written_rx.try_recv() {
                written_second_round.push(data);
            }
        }

        sleep(Duration::from_secs(1)).await;

        log_watcher_channel
            .send(LogWatcherSignal::Close)
            .await
            .unwrap();

        while let Ok(data) = log_watcher.try_recv() {
            for line in std::str::from_utf8(&data).unwrap().split("\n") {
                if line.len() > 0 {
                    read_second_round.push(format!("{}\n", line));
                }
            }
        }

        println!(
            "first round len: {} \nsecond round len: {}",
            read_first_round.len(),
            read_second_round.len()
        );
        for idx in 0..read_first_round.len() {
            assert_eq!(read_first_round[idx], written_first_round[idx]);
        }

        for idx in 0..read_second_round.len() {
            assert_eq!(read_second_round[idx], written_second_round[idx]);
        }
    }

    #[async_std::test]
    async fn pretty() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let count = 500;
        let mut test_writer = TestWriter::new("test_data", "test_single.txt", 1, count).await;

        let mut p_log_watcher = FileWatcher::new("test_data/test_single.txt").await;

        test_writer.start().await;
        p_log_watcher = p_log_watcher.next().await.unwrap();

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            if let Ok(data) = test_writer.written_rx.try_recv() {
                written.push(data);
            }
        }

        let channel = p_log_watcher.get_channel();

        let mut count = 0;
        loop {
            p_log_watcher = p_log_watcher.next().await.unwrap();
            if let Ok(msg) = channel.try_recv() {
                for line in std::str::from_utf8(&msg).unwrap().split("\n") {
                    if line.len() > 0 {
                        read.push(format!("{}\n", line));
                    }
                }
            }

            count += 1;

            if count > 10 {
                break;
            }
        }

        // assert_eq!(read.len(), count as usize);
        for idx in 0..read.len() {
            assert_eq!(read[idx], written[idx]);
        }
    }
}
