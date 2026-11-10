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
async fn sweep_orphan_drops_an_exit_deferred_behind_its_pending_root() {
    let worker = TraceNormalizerWorker::new(Arc::new(SystemGitBackend::new()));
    // A killed process leaves `start` and `exit` without the `atexit` that
    // would finalize them; only the connection-close sweep can reclaim both.
    for (event, ts) in [("start", 1), ("exit", 2)] {
        let frame = serde_json::json!({"event": event, "sid": "killed", "ts": ts});
        assert!(worker.ingest_payload(frame).await.unwrap().is_none());
    }
    worker.sweep_orphan("killed".to_string()).await.unwrap();
    let leftovers = worker
        .run(|normalizer| {
            let state = normalizer.state();
            Ok((state.pending.len(), state.deferred_exits.len()))
        })
        .await
        .unwrap();
    assert_eq!(leftovers, (0, 0));
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
