mod scenario;
#[cfg(test)]
mod tests {
    use crate::scenario::TestWriter;
    use async_log_watcher::LogWatcherSignal;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn it_works_read_to_end() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let count = 1000;
        let mut test_writer = TestWriter::new("test_data", "test_single.txt", 1, count).await;
        let mut log_watcher =
            async_log_watcher::LogWatcher::builder("test_data/test_single.txt").build();

        let future = log_watcher.spawn();

        tokio::task::spawn(async {
            future.await.unwrap();
        });

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(100)).await;
        }
        sleep(Duration::from_millis(2000)).await;

        while let Some(data) = test_writer.written_rx.recv().await {
            written.push(data);
        }

        assert_eq!(count as usize, written.len());

        while let Ok(data) = log_watcher.try_read_message() {
            for line in std::str::from_utf8(&data).unwrap().split('\n') {
                if !line.is_empty() {
                    println!("{:?}", line);
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
    async fn it_works_line_by_line() {
        let mut written: Vec<String> = vec![];
        let mut read: Vec<String> = vec![];
        let count = 1000;
        let mut test_writer = TestWriter::new("test_data", "test_single.txt", 1, count).await;
        let mut log_watcher = async_log_watcher::LogWatcher::builder("test_data/test_single.txt")
            .mode(async_log_watcher::LogReaderMode::NextLine)
            .build();

        let future = log_watcher.spawn();

        tokio::task::spawn(async {
            future.await.unwrap();
        });

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(100)).await;
        }
        sleep(Duration::from_millis(2000)).await;

        while let Some(data) = test_writer.written_rx.recv().await {
            written.push(data);
        }

        assert_eq!(count as usize, written.len());

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

        let mut log_watcher =
            async_log_watcher::LogWatcher::builder("test_data/test_reload.txt").build();

        let future = log_watcher.spawn();

        tokio::task::spawn(async {
            future.await.unwrap();
        });

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(100)).await;
        }

        sleep(Duration::from_millis(2000)).await;

        while let Ok(data) = test_writer.written_rx.try_recv() {
            written_first_round.push(data);
        }

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
            sleep(Duration::from_millis(100)).await;
        }

        sleep(Duration::from_millis(2000)).await;

        while let Ok(data) = test_writer.written_rx.try_recv() {
            written_second_round.push(data);
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

    #[tokio::test]
    async fn reload_working_line_by_line() {
        let mut written_first_round: Vec<String> = vec![];
        let mut written_second_round: Vec<String> = vec![];

        let mut read_first_round: Vec<String> = vec![];
        let mut read_second_round: Vec<String> = vec![];
        let count = 500;
        let mut test_writer = TestWriter::new("test_data", "test_reload.txt", 1, count).await;

        let mut log_watcher = async_log_watcher::LogWatcher::builder("test_data/test_reload.txt")
            .mode(async_log_watcher::LogReaderMode::NextLine)
            .build();

        let future = log_watcher.spawn();

        tokio::task::spawn(async {
            future.await.unwrap();
        });

        test_writer.start().await;

        while !test_writer.cancel_result.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(100)).await;
        }

        sleep(Duration::from_millis(2000)).await;

        while let Ok(data) = test_writer.written_rx.try_recv() {
            written_first_round.push(data);
        }

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
            sleep(Duration::from_millis(100)).await;
        }

        sleep(Duration::from_millis(2000)).await;

        while let Ok(data) = test_writer.written_rx.try_recv() {
            written_second_round.push(data);
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
