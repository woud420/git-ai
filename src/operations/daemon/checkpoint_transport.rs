use super::client_helpers::{
    DAEMON_CONTROL_MAX_FRAME_BYTES, checkpoint_control_response_timeout,
    checkpoint_control_timeout_uses_ci_or_test_budget, read_daemon_client_line,
    write_all_daemon_client_stream,
};
use super::transport_error::transport_error;
use super::{
    ActorDaemonCoordinator, ControlRequest, ControlResponse, DAEMON_CONTROL_CONNECT_TIMEOUT,
    DAEMON_CONTROL_RESPONSE_TIMEOUT, DaemonClientStream, open_local_socket_stream_with_timeout,
    set_daemon_client_stream_timeouts,
};
use crate::config::Config;
use crate::error::GitAiError;
use crate::model::checkpoint_capnp;
use crate::model::checkpoint_delivery::CheckpointDelivery;
use serde::Serialize;
use serde_json::json;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CAPNP_METHOD: &str = "checkpoint.deliver.capnp.v1";
const BODY_READ_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
pub enum CheckpointTransport {
    Json,
    Capnp,
}

pub fn send_checkpoint_delivery(
    socket_path: &Path,
    delivery: &CheckpointDelivery,
) -> Result<ControlResponse, GitAiError> {
    let transport = if Config::get().get_feature_flags().checkpoint_capnp {
        CheckpointTransport::Capnp
    } else {
        CheckpointTransport::Json
    };
    send_checkpoint_delivery_with_transport(socket_path, delivery, transport)
}

pub fn send_checkpoint_delivery_with_transport(
    socket_path: &Path,
    delivery: &CheckpointDelivery,
    transport: CheckpointTransport,
) -> Result<ControlResponse, GitAiError> {
    delivery
        .validate()
        .map_err(|error| GitAiError::PresetError(error.to_string()))?;
    let payload = match transport {
        CheckpointTransport::Json => json_delivery(delivery)?,
        CheckpointTransport::Capnp => checkpoint_capnp::encode_delivery(delivery)?,
    };
    if payload.len() > DAEMON_CONTROL_MAX_FRAME_BYTES {
        return Err(invalid_frame("checkpoint control frame exceeds byte limit"));
    }
    let response_timeout = checkpoint_control_response_timeout(
        &ControlRequest::CheckpointDeliverCapnpV1 { encoded_bytes: 0 },
        checkpoint_control_timeout_uses_ci_or_test_budget(),
    );
    let mut stream =
        open_local_socket_stream_with_timeout(socket_path, DAEMON_CONTROL_CONNECT_TIMEOUT)?;
    set_daemon_client_stream_timeouts(&mut stream, socket_path, response_timeout)?;
    let deadline = Instant::now() + response_timeout;
    let mut reader = BufReader::new(stream);
    match transport {
        CheckpointTransport::Json => {
            write_all_daemon_client_stream(reader.get_mut(), socket_path, &payload)?;
        }
        CheckpointTransport::Capnp => {
            let mut header = serde_json::to_vec(&ControlRequest::CheckpointDeliverCapnpV1 {
                encoded_bytes: payload.len() as u32,
            })?;
            header.push(b'\n');
            write_all_daemon_client_stream(reader.get_mut(), socket_path, &header)?;
            let ready = read_response(&mut reader, socket_path, deadline)?;
            if unknown_capnp_method(&ready) {
                // Refusal proves that no body was submitted. After body transfer,
                // only durable replay with the same delivery ID may retry.
                let legacy = json_delivery(delivery)?;
                if legacy.len() > DAEMON_CONTROL_MAX_FRAME_BYTES {
                    return Err(invalid_frame("checkpoint control frame exceeds byte limit"));
                }
                write_all_daemon_client_stream(reader.get_mut(), socket_path, &legacy)?;
            } else {
                if !ready.ok {
                    return Ok(ready);
                }
                if ready
                    .data
                    .as_ref()
                    .and_then(|data| data.get("checkpoint_capnp_v1"))
                    != Some(&json!("ready"))
                {
                    return Err(invalid_frame(
                        "invalid binary checkpoint readiness response",
                    ));
                }
                write_all_daemon_client_stream(reader.get_mut(), socket_path, &payload)?;
            }
        }
    }
    read_response(&mut reader, socket_path, deadline)
}

#[derive(Serialize)]
struct JsonDelivery<'a> {
    method: &'static str,
    params: JsonDeliveryParams<'a>,
}

#[derive(Serialize)]
struct JsonDeliveryParams<'a> {
    delivery: &'a CheckpointDelivery,
}

fn json_delivery(delivery: &CheckpointDelivery) -> Result<Vec<u8>, GitAiError> {
    let mut bytes = serde_json::to_vec(&JsonDelivery {
        method: "checkpoint.deliver",
        params: JsonDeliveryParams { delivery },
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn unknown_capnp_method(response: &ControlResponse) -> bool {
    !response.ok
        && response.error.as_deref().is_some_and(|error| {
            error.starts_with(&format!(
                "invalid control request: unknown variant `{CAPNP_METHOD}`"
            ))
        })
}

fn read_response(
    reader: &mut BufReader<DaemonClientStream>,
    socket_path: &Path,
    deadline: Instant,
) -> Result<ControlResponse, GitAiError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| {
            transport_error(
                std::io::ErrorKind::TimedOut,
                "checkpoint response timed out".into(),
            )
        })?;
    set_daemon_client_stream_timeouts(reader.get_mut(), socket_path, remaining)?;
    let line = read_daemon_client_line(reader, socket_path, remaining)?;
    serde_json::from_str(line.trim()).map_err(GitAiError::from)
}

pub(crate) fn handle_capnp_checkpoint<R: Read + Write>(
    reader: &mut BufReader<R>,
    coordinator: &Arc<ActorDaemonCoordinator>,
    runtime: &tokio::runtime::Handle,
    encoded_bytes: usize,
    set_read_timeout: &mut impl FnMut(&mut R, Option<Duration>) -> io::Result<()>,
) -> Result<(), GitAiError> {
    if encoded_bytes == 0 || encoded_bytes > DAEMON_CONTROL_MAX_FRAME_BYTES {
        return write_response(
            reader.get_mut(),
            &ControlResponse::err("invalid binary checkpoint frame length"),
        );
    }
    let Some(_admission) = coordinator.control_admission.checkpoint(encoded_bytes) else {
        return write_response(
            reader.get_mut(),
            &ControlResponse::err("daemon checkpoint admission is full; retry delivery"),
        );
    };
    let received = (|| -> Result<Vec<u8>, GitAiError> {
        set_read_timeout(reader.get_mut(), Some(BODY_READ_POLL_INTERVAL))?;
        write_response(
            reader.get_mut(),
            &ControlResponse::ok(None, Some(json!({ "checkpoint_capnp_v1": "ready" }))),
        )?;
        Ok(read_checkpoint_body(
            reader,
            coordinator,
            encoded_bytes,
            set_read_timeout,
        )?)
    })();
    // The same connection may return to ordinary JSON requests after this body.
    let restored = set_read_timeout(reader.get_mut(), None);
    let body = received?;
    restored?;
    let delivery = match checkpoint_capnp::decode_delivery(&body) {
        Ok(delivery) => delivery,
        Err(error) => {
            write_response(reader.get_mut(), &ControlResponse::err(error.to_string()))?;
            return Err(error);
        }
    };
    drop(body);
    let response = match runtime.block_on(coordinator.ingest_checkpoint_delivery(delivery)) {
        Ok(response) => response,
        Err(error) => ControlResponse::err(error.to_string()),
    };
    write_response(reader.get_mut(), &response)
}

fn read_checkpoint_body<R: Read>(
    reader: &mut BufReader<R>,
    coordinator: &ActorDaemonCoordinator,
    encoded_bytes: usize,
    set_read_timeout: &mut impl FnMut(&mut R, Option<Duration>) -> io::Result<()>,
) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + DAEMON_CONTROL_RESPONSE_TIMEOUT;
    let mut body = vec![0; encoded_bytes];
    let mut received = 0;
    while received < body.len() {
        if coordinator.is_shutting_down() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "daemon is shutting down before binary checkpoint body completed",
            ));
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(body_timeout)?;
        if remaining < BODY_READ_POLL_INTERVAL {
            set_read_timeout(reader.get_mut(), Some(remaining))?;
        }
        match reader.read(&mut body[received..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "binary checkpoint body closed before expected bytes",
                ));
            }
            Ok(count) => received += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    if Instant::now() >= deadline {
        return Err(body_timeout());
    }
    Ok(body)
}

fn body_timeout() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "binary checkpoint body receive timed out",
    )
}

pub(crate) fn write_response<W: Write>(
    writer: &mut W,
    response: &ControlResponse,
) -> Result<(), GitAiError> {
    serde_json::to_writer(&mut *writer, response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn invalid_frame(message: &str) -> GitAiError {
    transport_error(std::io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_unknown_method_refusal_allows_legacy_fallback() {
        assert!(unknown_capnp_method(&ControlResponse::err(format!(
            "invalid control request: unknown variant `{CAPNP_METHOD}`, expected one of `ping`"
        ))));
        for error in [
            "daemon checkpoint admission is full; retry delivery",
            "invalid control request: unknown variant `checkpoint.deliver.other`",
            "checkpoint application failed",
        ] {
            assert!(!unknown_capnp_method(&ControlResponse::err(error)));
        }
        assert!(!unknown_capnp_method(&ControlResponse::ok(None, None)));
    }

    #[test]
    fn generic_sender_cannot_return_binary_readiness_as_a_delivery_ack() {
        let error = super::super::send_control_request(
            Path::new("unused-control-socket"),
            &ControlRequest::CheckpointDeliverCapnpV1 { encoded_bytes: 8 },
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires the checkpoint transport sender")
        );
    }
}
