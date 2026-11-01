use super::{Error, OpenOutcome, Portal};
use std::future::ready;

#[test]
fn failed_default_open_offers_the_application_chooser() {
    let mut requests = Vec::new();
    let outcome = futures_lite::future::block_on(Portal::default_or_choose(|ask| {
        requests.push(ask);
        ready(if ask {
            Ok(OpenOutcome::Opened)
        } else {
            Err(Error::Request {
                path: "/document".into(),
                source: ashpd::Error::NoResponse,
            })
        })
    }))
    .expect("the chosen application opens the document");
    assert_eq!(requests, [false, true]);
    assert_eq!(outcome, OpenOutcome::Opened);
}

#[test]
fn success_and_cancellation_do_not_reopen_the_chooser() {
    for response in [OpenOutcome::Opened, OpenOutcome::Cancelled] {
        let mut requests = Vec::new();
        let outcome = futures_lite::future::block_on(Portal::default_or_choose(|ask| {
            requests.push(ask);
            ready(Ok(response))
        }))
        .expect("a successful portal response");
        assert_eq!(outcome, response);
        assert_eq!(requests, [false]);
    }
}

#[test]
fn both_failures_remain_available_to_the_error_report() {
    let error = futures_lite::future::block_on(Portal::default_or_choose(|ask| {
        ready(Err(Error::Request {
            path: "/document".into(),
            source: if ask {
                ashpd::Error::RequiresVersion(2, 1)
            } else {
                ashpd::Error::NoResponse
            },
        }))
    }))
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
