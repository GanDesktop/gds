use super::*;
use std::{io::Write, os::unix::net::UnixStream, time::Duration};

#[test]
fn pipe_reads_require_eof_and_enforce_size_and_deadline() {
    let (reader, mut writer) = UnixStream::pair().expect("local pipe");
    writer
        .write_all(b"\0\xff\r\n")
        .expect("binary offer payload");
    drop(writer);
    assert_eq!(
        Clipboard::new(Duration::from_secs(1), 4)
            .read_pipe(reader, Instant::now() + Duration::from_secs(1))
            .expect("complete offer"),
        b"\0\xff\r\n"
    );

    let (reader, mut writer) = UnixStream::pair().expect("local pipe");
    writer.write_all(b"abcde").expect("oversized offer");
    assert!(matches!(
        Clipboard::new(Duration::from_secs(1), 4)
            .read_pipe(reader, Instant::now() + Duration::from_secs(1)),
        Err(Error::TooLarge)
    ));

    let (reader, _writer) = UnixStream::pair().expect("stalled offer");
    assert!(matches!(
        Clipboard::new(Duration::from_millis(10), 4)
            .read_pipe(reader, Instant::now() + Duration::from_millis(10)),
        Err(Error::Timeout)
    ));
}
