use super::enums;
use crate::checkpoint_capnp as wire;
use crate::model::checkpoint_delivery::CheckpointDelivery;
use crate::model::checkpoint_request::{
    BaseCommit, CheckpointFile, CheckpointRequest, StreamSource,
};
use std::path::Path;

pub(super) fn build_message(
    delivery: &CheckpointDelivery,
) -> capnp::Result<capnp::message::Builder<capnp::message::HeapAllocator>> {
    check_text_budget(delivery)?;
    let mut message = capnp::message::Builder::new_default();
    let mut root = message.init_root::<wire::checkpoint_delivery::Builder<'_>>();
    root.set_schema_version(delivery.schema_version);
    root.set_delivery_id(delivery.delivery_id.as_str());
    root.set_batch_id(delivery.batch_id.as_str());
    root.set_batch_ordinal(delivery.batch_ordinal);
    root.set_captured_at_unix_ms(delivery.captured_at_unix_ms);
    root.set_producer_version(delivery.producer_version.as_str());
    write_request(root.init_request(), &delivery.request)?;
    Ok(message)
}

fn write_request(
    mut target: wire::checkpoint_request::Builder<'_>,
    request: &CheckpointRequest,
) -> capnp::Result<()> {
    target.set_trace_id(request.trace_id.as_str());
    target.set_checkpoint_kind(enums::encode_kind(request.checkpoint_kind));
    target.set_path_role(enums::encode_role(request.path_role));
    if let Some(agent) = &request.agent_id {
        let mut encoded = target.reborrow().init_agent_id();
        encoded.set_tool(agent.tool.as_str());
        encoded.set_id(agent.id.as_str());
        encoded.set_model(agent.model.as_str());
    }
    let mut files = target.reborrow().init_files(request.files.len() as u32);
    for (index, file) in request.files.iter().enumerate() {
        write_file(files.reborrow().get(index as u32), file)?;
    }
    if let Some(source) = &request.stream_source {
        write_source(target.reborrow().init_stream_source(), source)?;
    }
    let mut metadata = target
        .reborrow()
        .init_metadata(request.metadata.len() as u32);
    for (index, (key, value)) in request.metadata.iter().enumerate() {
        let mut entry = metadata.reborrow().get(index as u32);
        entry.set_key(key.as_str());
        entry.set_value(value.as_str());
    }
    write_optional(target.init_delivery_id(), request.delivery_id.as_deref());
    Ok(())
}

fn write_file(
    mut target: wire::checkpoint_file::Builder<'_>,
    file: &CheckpointFile,
) -> capnp::Result<()> {
    target.set_path(path_text(&file.path)?);
    target.set_repo_work_dir(path_text(&file.repo_work_dir)?);
    write_optional(target.reborrow().init_content(), file.content.as_deref());
    let mut base = target.init_base_commit();
    match &file.base_commit {
        BaseCommit::Initial => base.set_initial(()),
        BaseCommit::Sha(sha) => base.set_sha(sha.as_str()),
    }
    Ok(())
}

fn write_source(
    mut target: wire::stream_source::Builder<'_>,
    source: &StreamSource,
) -> capnp::Result<()> {
    target.set_path(path_text(&source.path)?);
    target.set_format(enums::encode_format(source.format));
    target.set_session_id(source.session_id.as_str());
    target.set_external_session_id(source.external_session_id.as_str());
    write_optional(
        target.init_external_parent_session_id(),
        source.external_parent_session_id.as_deref(),
    );
    Ok(())
}

fn write_optional(mut target: wire::optional_text::Builder<'_>, value: Option<&str>) {
    match value {
        None => target.set_absent(()),
        Some(value) => target.set_value(value),
    }
}

fn path_text(path: &Path) -> capnp::Result<&str> {
    path.to_str()
        .ok_or_else(|| capnp::Error::failed("checkpoint path is not UTF-8".into()))
}

// Reject oversized owned inputs before duplicating their contents into a
// message arena. The final serialized-size check also accounts for wire layout.
fn check_text_budget(delivery: &CheckpointDelivery) -> capnp::Result<()> {
    let mut budget = TextBudget(0);
    for text in [
        &delivery.delivery_id,
        &delivery.batch_id,
        &delivery.producer_version,
        &delivery.request.trace_id,
    ] {
        budget.add(text)?;
    }
    if let Some(agent) = &delivery.request.agent_id {
        for text in [&agent.tool, &agent.id, &agent.model] {
            budget.add(text)?;
        }
    }
    for file in &delivery.request.files {
        budget.add(path_text(&file.path)?)?;
        budget.add(path_text(&file.repo_work_dir)?)?;
        budget.add(file.content.as_deref().unwrap_or_default())?;
        if let BaseCommit::Sha(sha) = &file.base_commit {
            budget.add(sha)?;
        }
    }
    if let Some(source) = &delivery.request.stream_source {
        for text in [
            path_text(&source.path)?,
            &source.session_id,
            &source.external_session_id,
            source
                .external_parent_session_id
                .as_deref()
                .unwrap_or_default(),
        ] {
            budget.add(text)?;
        }
    }
    for (key, value) in &delivery.request.metadata {
        budget.add(key)?;
        budget.add(value)?;
    }
    budget.add(delivery.request.delivery_id.as_deref().unwrap_or_default())
}

struct TextBudget(usize);

impl TextBudget {
    fn add(&mut self, text: &str) -> capnp::Result<()> {
        if text.len() > super::MAX_ENCODED_BYTES.saturating_sub(self.0) {
            return Err(capnp::Error::failed(
                "checkpoint payload exceeds size limit".into(),
            ));
        }
        self.0 += text.len();
        Ok(())
    }
}
