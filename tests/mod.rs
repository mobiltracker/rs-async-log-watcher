mod scenario;
#[cfg(test)]
mod tests {
    use crate::scenario::TestWriter;
    use async_log_watcher::LogWatcherSignal;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn it_works() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let count = 500;
        let mut test_writer = TestWriter::new("test_data", "test_single.txt", 1, count).await;
        let mut log_watcher = async_log_watcher::LogWatcher::new("test_data/test_single.txt");

        let _future = log_watcher.spawn(false).await;

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            if let Ok(data) = test_writer.written_rx.try_recv() {
                written.push(data);
            }
        }

        sleep(Duration::from_secs(1)).await;

        log_watcher
            .send_signal(LogWatcherSignal::Close)
            .await
            .unwrap();

        while let Ok(data) = log_watcher.try_read_message() {
            for line in std::str::from_utf8(&data).unwrap().split('\n') {
                if !line.is_empty() {
                    println!("{:?}", line);
                    read.push(format!("{}\n", line));
                }
            }
        }

        while let Ok(data) = log_watcher.try_read_message() {
            for line in std::str::from_utf8(&data).unwrap().split('\n') {
                if !line.is_empty() {
                    read.push(format!("{}\n", line));
                }
            }
        }

        assert_eq!(read.len(), count as usize);
        for idx in 0..read.len() {
            assert_eq!(read[idx], written[idx]);
        }
    }

    #[tokio::test]
    async fn reload_working() {
        let mut written_first_round: Vec<String> = vec![];
        let mut written_second_round: Vec<String> = vec![];

        let mut read_first_round: Vec<String> = vec![];
        let mut read_second_round: Vec<String> = vec![];
        let count = 500;
        let mut test_writer = TestWriter::new("test_data", "test_reload.txt", 1, count).await;

        let mut log_watcher = async_log_watcher::LogWatcher::new("test_data/test_reload.txt");
        log_watcher.spawn(false).await.unwrap();

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            while let Ok(data) = test_writer.written_rx.try_recv() {
                written_first_round.push(data);
            }
        }

        sleep(Duration::from_millis(1000)).await;

        while let Ok(data) = log_watcher.try_read_message() {
            for line in std::str::from_utf8(&data).unwrap().split('\n') {
                if !line.is_empty() {
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

        log_watcher
            .send_signal(LogWatcherSignal::Close)
            .await
            .unwrap();

        while let Ok(data) = log_watcher.try_read_message() {
            for line in std::str::from_utf8(&data).unwrap().split('\n') {
                if !line.is_empty() {
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
}
