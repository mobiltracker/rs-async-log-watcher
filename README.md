# rs-async-log-watcher


Async log watcher using [tokio::fs](https://docs.rs/tokio/latest/tokio/fs/index.html) (not 'truly' async like io_uring as tokio uses worker threads to handle async fs io operations). 

Handles file deletion without crashing somewhat nicely. As long as there is a file in the providade path, it should work (even if the file is deleted and recreated after some time). 
If you need more functionaly you probably should take a look at [notify](docs.rs/notify) and implement it yourself using this library.

Compatible with Unix/Windows. I do not have benchmarks (and theres a lot of possible improvements), but everything should be pretty resource efficient. 
It should be possible to spawn thousand of log watchers without worrying to much about it (tokio can manage the blocking threads very well).

### Examples

## TL;DR
``` rust
#[tokio::main]
async fn main() {
  let mut log_watcher = async_log_watcher::LogWatcher::new("foobar.txt");

  // spawn() returns a task handle that must be awaited somewhere to run the log reading loop
  let log_watcher_handle = log_watcher.spawn(false);
  
  // New task or thread or whatever. I'd prefer to not spawn tasks or include a tokio/rt dependency
  // so the caller is responsable to drive the future somewhere.
  tokio::task::spawn(async {
  // This is going to run the log reading loop.
    log_watcher_handle.await.unwrap();
  });
  
  // log_watcher.try_read_message() for non blocking reading
  while let Some(data) = log_watcher.read_message().await {
    for line in std::str::from_utf8(&data).unwrap().split('\n') {
        println!("{}", line);
    }
  }
  
  // Close log watcher by sending some command. You can also reload the file or change the file being read
  log_watcher
        .send_signal(LogWatcherSignal::Close)
        .await
        .unwrap();

}


```
