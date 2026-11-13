use super::{Error, OpenOutcome, Portal};
use std::{
    future::ready,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::AsRawFd,
    path::Path,
};

#[test]
fn failed_default_open_offers_the_application_chooser() {
    let file = tempfile::tempfile().expect("temporary file");
    let mut requests = Vec::new();
    let outcome = futures_lite::future::block_on(Portal::default_or_choose(
        Path::new("/document"),
        &file,
        |ask| {
            requests.push(ask);
            ready(if ask {
                Ok(OpenOutcome::Opened)
            } else {
                Err(Error::Request {
                    path: "/document".into(),
                    source: ashpd::Error::NoResponse,
                })
            })
        },
    ))
    .expect("the chosen application opens the document");
    assert_eq!(requests, [false, true]);
    assert_eq!(outcome, OpenOutcome::Opened);
}

#[test]
fn success_and_cancellation_do_not_reopen_the_chooser() {
    let file = tempfile::tempfile().expect("temporary file");
    for response in [OpenOutcome::Opened, OpenOutcome::Cancelled] {
        let mut requests = Vec::new();
        let outcome = futures_lite::future::block_on(Portal::default_or_choose(
            Path::new("/document"),
            &file,
            |ask| {
                requests.push(ask);
                ready(Ok(response))
            },
        ))
        .expect("a successful portal response");
        assert_eq!(outcome, response);
        assert_eq!(requests, [false]);
    }
}

#[test]
fn both_failures_remain_available_to_the_error_report() {
    let file = tempfile::tempfile().expect("temporary file");
    let error = futures_lite::future::block_on(Portal::default_or_choose(
        Path::new("/document"),
        &file,
        |ask| {
            ready(Err(Error::Request {
                path: "/document".into(),
                source: if ask {
                    ashpd::Error::RequiresVersion(2, 1)
                } else {
                    ashpd::Error::NoResponse
                },
            }))
        },
    ))
    .expect_err("neither open request succeeded");
    let Error::DefaultAndChooser { default, chooser } = error else {
        panic!("both typed errors are retained");
    };
    assert!(matches!(
        *default,
        Error::Request {
            source: ashpd::Error::NoResponse,
            ..
        }
    ));
    assert!(matches!(
        *chooser,
        Error::Request {
            source: ashpd::Error::RequiresVersion(2, 1),
            ..
        }
    ));
}

#[test]
fn fallback_chooser_rewinds_and_reuses_the_same_open_file() {
    let mut file = tempfile::tempfile().expect("temporary file");
    file.write_all(b"document bytes").expect("write payload");
    file.seek(SeekFrom::Start(0)).expect("rewind payload");

    let mut observed_fds = Vec::new();
    let outcome = futures_lite::future::block_on(Portal::default_or_choose(
        Path::new("/document"),
        &file,
        |ask| {
            observed_fds.push(file.as_raw_fd());
            let mut reader = &file;
            if !ask {
                let mut contents = Vec::new();
                reader.read_to_end(&mut contents).expect("read default fd");
                assert_eq!(contents, b"document bytes");
                assert_eq!(reader.stream_position().expect("eof offset"), 14);
                return ready(Err(Error::Request {
                    path: "/document".into(),
                    source: ashpd::Error::NoResponse,
                }));
            }
            assert_eq!(reader.stream_position().expect("rewound offset"), 0);
            let mut contents = Vec::new();
            reader.read_to_end(&mut contents).expect("read chooser fd");
            assert_eq!(contents, b"document bytes");
            ready(Ok(OpenOutcome::Opened))
        },
    ))
    .expect("chooser reuses the rewound descriptor");

    assert_eq!(outcome, OpenOutcome::Opened);
    assert_eq!(observed_fds, [file.as_raw_fd(), file.as_raw_fd()]);
}
