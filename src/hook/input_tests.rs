use super::*;
use std::{
    io::Write,
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::{Duration, Instant},
};
#[test]
fn ready_payload_is_read_even_after_scheduler_delay() {
    let mut file = tempfile::tempfile().unwrap();
    let payload = b"{\"hook_event_name\":\"Stop\"}";
    file.write_all(payload).unwrap();
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).unwrap();
    let fd = file.as_raw_fd();
    let (bytes, _) = read_fd(&mut file, fd, Instant::now() - Duration::from_millis(1)).unwrap();
    assert_eq!(bytes, payload);
    assert!(serde_json::from_slice::<Value>(&bytes).is_ok());
}
#[test]
fn progressing_writer_is_read_to_eof_even_past_one_second() {
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    let thread = std::thread::spawn(move || {
        for part in [b"one".as_slice(), b"two", b"three", b"four"] {
            writer.write_all(part).unwrap();
            std::thread::sleep(Duration::from_millis(350));
        }
    });
    let fd = reader.as_raw_fd();
    let result = read_fd(&mut reader, fd, Instant::now() + INPUT_IDLE);
    thread.join().unwrap();
    let (bytes, expired) = result.unwrap();
    assert_eq!(bytes, b"onetwothreefour");
    assert!(!expired);
}

#[test]
fn silent_open_input_expires_only_after_one_second() {
    let (mut reader, _writer) = UnixStream::pair().unwrap();
    let fd = reader.as_raw_fd();
    let started = Instant::now();
    let (bytes, expired) = read_fd(&mut reader, fd, started + INPUT_IDLE).unwrap();
    assert!(bytes.is_empty() && expired);
    assert!(started.elapsed() >= INPUT_IDLE);
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn interrupted_read_does_not_discard_ready_data_after_deadline() {
    struct InterruptedOnce {
        file: std::fs::File,
        interrupted: bool,
    }
    impl Read for InterruptedOnce {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if !std::mem::replace(&mut self.interrupted, true) {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.file.read(bytes)
        }
    }
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"ready").unwrap();
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).unwrap();
    let fd = file.as_raw_fd();
    let mut reader = InterruptedOnce {
        file,
        interrupted: false,
    };
    let (bytes, expired) =
        read_fd(&mut reader, fd, Instant::now() - Duration::from_millis(1)).unwrap();
    assert_eq!(bytes, b"ready");
    assert!(!expired);
}
