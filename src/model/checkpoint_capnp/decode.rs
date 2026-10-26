use super::enums;
use crate::checkpoint_capnp as wire;
use crate::model::checkpoint_delivery::{
    CHECKPOINT_DELIVERY_MAX_FILES, CHECKPOINT_DELIVERY_MAX_METADATA_ENTRIES,
    CHECKPOINT_DELIVERY_MAX_METADATA_KEY_BYTES, CHECKPOINT_DELIVERY_MAX_METADATA_VALUE_BYTES,
    CHECKPOINT_DELIVERY_MAX_PATH_BYTES, CheckpointDelivery,
};
use crate::model::checkpoint_request::{
    BaseCommit, CheckpointFile, CheckpointRequest, StreamSource,
};
use crate::model::working_log::AgentId;
use std::collections::{HashMap, hash_map::Entry};
use std::path::PathBuf;

pub(super) fn read_delivery(
    source: wire::checkpoint_delivery::Reader<'_>,
) -> capnp::Result<CheckpointDelivery> {
    Ok(CheckpointDelivery {
        schema_version: source.get_schema_version(),
        delivery_id: text(source.get_delivery_id()?)?,
        batch_id: text(source.get_batch_id()?)?,
        batch_ordinal: source.get_batch_ordinal(),
        captured_at_unix_ms: source.get_captured_at_unix_ms(),
        producer_version: text(source.get_producer_version()?)?,
        request: read_request(source.get_request()?)?,
    })
}

fn read_request(source: wire::checkpoint_request::Reader<'_>) -> capnp::Result<CheckpointRequest> {
    let files = source.get_files()?;
    check_len(files.len() as usize, CHECKPOINT_DELIVERY_MAX_FILES, "files")?;
    let mut decoded_files = Vec::with_capacity(files.len() as usize);
    for file in files {
        decoded_files.push(read_file(file)?);
    }
    let agent_id = if source.has_agent_id() {
        let agent = source.get_agent_id()?;
        Some(AgentId {
            tool: text(agent.get_tool()?)?,
            id: text(agent.get_id()?)?,
            model: text(agent.get_model()?)?,
        })
    } else {
        None
    };
    let stream_source = if source.has_stream_source() {
        Some(read_source(source.get_stream_source()?)?)
    } else {
        None
    };
    let metadata = source.get_metadata()?;
    check_len(
        metadata.len() as usize,
        CHECKPOINT_DELIVERY_MAX_METADATA_ENTRIES,
        "metadata entries",
    )?;
    let mut decoded_metadata = HashMap::with_capacity(metadata.len() as usize);
    for item in metadata {
        let key = item.get_key()?.to_str()?;
        check_len(
            key.len(),
            CHECKPOINT_DELIVERY_MAX_METADATA_KEY_BYTES,
            "metadata key",
        )?;
        let value = item.get_value()?.to_str()?;
        check_len(
            value.len(),
            CHECKPOINT_DELIVERY_MAX_METADATA_VALUE_BYTES,
            "metadata value",
        )?;
        match decoded_metadata.entry(key.to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(value.to_owned());
            }
            Entry::Occupied(_) => {
                return Err(capnp::Error::failed(
                    "duplicate checkpoint metadata key".into(),
                ));
            }
        }
    }
    Ok(CheckpointRequest {
        trace_id: text(source.get_trace_id()?)?,
        checkpoint_kind: enums::decode_kind(source.get_checkpoint_kind()?),
        agent_id,
        files: decoded_files,
        path_role: enums::decode_role(source.get_path_role()?),
        stream_source,
        metadata: decoded_metadata,
        delivery_id: optional_text(source.get_delivery_id()?)?,
    })
}

fn read_file(source: wire::checkpoint_file::Reader<'_>) -> capnp::Result<CheckpointFile> {
    let base_commit = match source.get_base_commit()?.which()? {
        wire::base_commit::Initial(()) => BaseCommit::Initial,
        wire::base_commit::Sha(sha) => BaseCommit::Sha(text(sha?)?),
    };
    Ok(CheckpointFile {
        path: path(source.get_path()?)?,
        content: optional_text(source.get_content()?)?,
        repo_work_dir: path(source.get_repo_work_dir()?)?,
        base_commit,
    })
}

fn read_source(source: wire::stream_source::Reader<'_>) -> capnp::Result<StreamSource> {
    Ok(StreamSource {
        path: path(source.get_path()?)?,
        format: enums::decode_format(source.get_format()?),
        session_id: text(source.get_session_id()?)?,
        external_session_id: text(source.get_external_session_id()?)?,
        external_parent_session_id: optional_text(source.get_external_parent_session_id()?)?,
    })
}

fn optional_text(source: wire::optional_text::Reader<'_>) -> capnp::Result<Option<String>> {
    match source.which()? {
        wire::optional_text::Absent(()) => Ok(None),
        wire::optional_text::Value(value) => Ok(Some(text(value?)?)),
    }
}

fn text(source: capnp::text::Reader<'_>) -> capnp::Result<String> {
    Ok(source.to_str()?.to_owned())
}

fn path(source: capnp::text::Reader<'_>) -> capnp::Result<PathBuf> {
    let path = source.to_str()?;
    check_len(path.len(), CHECKPOINT_DELIVERY_MAX_PATH_BYTES, "path")?;
    Ok(PathBuf::from(path))
}

fn check_len(actual: usize, limit: usize, field: &str) -> capnp::Result<()> {
    if actual > limit {
        return Err(capnp::Error::failed(format!(
            "checkpoint {field} exceeds limit {limit}"
        )));
    }
    Ok(())
}
