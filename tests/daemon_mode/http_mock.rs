use super::*;

#[test]
fn mock_api_waits_for_a_request_on_an_accepted_nonblocking_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind request fixture");
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).expect("connect request");
    let (mut accepted, _) = listener.accept().expect("accept request");
    accepted.set_nonblocking(true).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (request_tx, request_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        request_tx.send(read_http_request(&mut accepted)).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let early = request_rx.recv_timeout(Duration::from_millis(100));
    let sent =
        client.write_all(b"POST /worker/metrics/upload HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}");
    reader.join().unwrap();
    assert!(
        matches!(early, Err(mpsc::RecvTimeoutError::Timeout)),
        "the mock disconnected before the client sent its request: {early:?}"
    );
    sent.expect("send delayed request");
    assert_eq!(
        request_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Some(("/worker/metrics/upload".to_string(), b"{}".to_vec()))
    );
}
