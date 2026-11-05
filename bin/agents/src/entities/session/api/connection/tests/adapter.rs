use super::*;

#[tokio::test]
async fn request_retains_rpc_code_and_details_through_anyhow_context() {
    let details = vec![0, 1, 255, 7];
    let status = Status::with_details(
        Code::PermissionDenied,
        "daemon diagnostic",
        details.clone().into(),
    );
    let failure =
        request::<()>(async { Err(anyhow::Error::from(status).context("external client")) })
            .await
            .expect_err("failed request");
    assert!(
        matches!(failure, Failure::Rpc { code: Code::PermissionDenied, ref message, details: ref actual, .. }
        if message == "daemon diagnostic" && actual == &details)
    );
    assert!(failure.presentation().contains("daemon diagnostic"));
}
