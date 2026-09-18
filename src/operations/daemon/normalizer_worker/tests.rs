use super::*;

#[tokio::test]
async fn normalizer_panic_reaches_the_caller_and_later_work_can_run() {
    let worker = Arc::new(TraceNormalizerWorker::new(
        Arc::new(SystemGitBackend::new()),
    ));
    let panicking_worker = Arc::clone(&worker);
    let error = tokio::spawn(async move {
        panicking_worker
            .run::<()>(|_| panic!("normalizer test panic"))
            .await
    })
    .await
    .unwrap_err();
    assert_eq!(
        error.into_panic().downcast_ref::<&str>(),
        Some(&"normalizer test panic")
    );
    assert_eq!(worker.run(|_| Ok(7)).await.unwrap(), 7);
}

#[tokio::test]
async fn normalizer_errors_preserve_their_structural_variant() {
    let worker = TraceNormalizerWorker::new(Arc::new(SystemGitBackend::new()));
    let error = worker
        .run::<()>(|_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()))
        .await
        .unwrap_err();
    assert!(
        matches!(error, GitAiError::IoError(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
    );
}
