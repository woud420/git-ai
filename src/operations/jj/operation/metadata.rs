use super::wire::{Budget, Fields, boolean, singular};
use super::{JjDecodeError, MAX_OPERATION_ATTRIBUTES, MAX_OPERATION_METADATA_BYTES};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Timestamp {
    pub millis: i64,
    pub tz_offset: i32,
}

#[derive(Default)]
pub(super) struct Metadata<'a> {
    pub start: Timestamp,
    pub end: Timestamp,
    pub description: &'a str,
    pub hostname: &'a str,
    pub username: &'a str,
    pub is_snapshot: bool,
    pub workspace_name: Option<&'a str>,
    pub attributes: BTreeMap<&'a str, &'a str>,
}

pub(super) fn decode(bytes: &[u8]) -> Result<Metadata<'_>, JjDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut start = None;
    let mut end = None;
    let mut description = None;
    let mut hostname = None;
    let mut username = None;
    let mut snapshot = None;
    let mut workspace = None;
    let mut attributes = BTreeMap::new();
    let mut bytes_budget =
        Budget::new(MAX_OPERATION_METADATA_BYTES, "metadata byte limit exceeded");
    let mut attribute_budget =
        Budget::new(MAX_OPERATION_ATTRIBUTES, "attribute count limit exceeded");
    while let Some(field) = fields.next(&[
        (1, 2),
        (2, 2),
        (3, 2),
        (4, 2),
        (5, 2),
        (6, 2),
        (7, 0),
        (8, 2),
    ])? {
        match field.tag {
            1 => singular(&mut start, field.bytes()?)?,
            2 => singular(&mut end, field.bytes()?)?,
            3 => singular(&mut description, string(field.bytes()?, &mut bytes_budget)?)?,
            4 => singular(&mut hostname, string(field.bytes()?, &mut bytes_budget)?)?,
            5 => singular(&mut username, string(field.bytes()?, &mut bytes_budget)?)?,
            6 => {
                attribute_budget.take(1)?;
                let (key, value) = attribute(field.bytes()?, &mut bytes_budget)?;
                if attributes.contains_key(key) {
                    return Err(JjDecodeError("duplicate attribute key"));
                }
                attributes.insert(key, value);
            }
            7 => singular(&mut snapshot, boolean(field.varint()?)?)?,
            8 => singular(&mut workspace, string(field.bytes()?, &mut bytes_budget)?)?,
            _ => return Err(JjDecodeError("unknown metadata field")),
        }
    }
    Ok(Metadata {
        start: timestamp(start.unwrap_or_default())?,
        end: timestamp(end.unwrap_or_default())?,
        description: description.unwrap_or_default(),
        hostname: hostname.unwrap_or_default(),
        username: username.unwrap_or_default(),
        is_snapshot: snapshot.unwrap_or_default(),
        workspace_name: workspace,
        attributes,
    })
}

fn string<'a>(bytes: &'a [u8], budget: &mut Budget) -> Result<&'a str, JjDecodeError> {
    budget.take(bytes.len())?;
    std::str::from_utf8(bytes).map_err(|_| JjDecodeError("invalid utf8 metadata"))
}

fn attribute<'a>(
    bytes: &'a [u8],
    budget: &mut Budget,
) -> Result<(&'a str, &'a str), JjDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut key = None;
    let mut value = None;
    while let Some(field) = fields.next(&[(1, 2), (2, 2)])? {
        match field.tag {
            1 => singular(&mut key, field.bytes()?)?,
            2 => singular(&mut value, field.bytes()?)?,
            _ => return Err(JjDecodeError("unknown attribute field")),
        }
    }
    Ok((
        string(key.unwrap_or_default(), budget)?,
        string(value.unwrap_or_default(), budget)?,
    ))
}

fn timestamp(bytes: &[u8]) -> Result<Timestamp, JjDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut millis = None;
    let mut tz_offset = None;
    while let Some(field) = fields.next(&[(1, 0), (2, 0)])? {
        match field.tag {
            1 => singular(&mut millis, field.varint()?)?,
            2 => singular(&mut tz_offset, field.varint()?)?,
            _ => return Err(JjDecodeError("unknown timestamp field")),
        }
    }
    // Prost's int64/int32 decoding casts the protobuf u64 without range normalization.
    Ok(Timestamp {
        millis: millis.unwrap_or_default() as i64,
        tz_offset: tz_offset.unwrap_or_default() as i32,
    })
}
