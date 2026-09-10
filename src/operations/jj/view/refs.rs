use super::super::wire::{Fields, identity, singular};
use super::JjViewDecodeError;
use super::types::{Budgets, Target};

pub(super) fn target<'a>(
    bytes: Option<&'a [u8]>,
    budgets: &mut Budgets,
    semantic: bool,
) -> Result<Target<'a>, JjViewDecodeError> {
    let Some(bytes) = bytes else {
        // The caller has already charged the containing entry; absence has no term message.
        return Ok(vec![None]);
    };
    let mut fields = Fields::new(bytes);
    let mut conflict = None;
    while let Some(field) = fields.next(&[(1, 2), (2, 2), (3, 2)])? {
        match field.tag {
            1 | 2 => return Err(JjViewDecodeError("unsupported legacy ref target")),
            3 => singular(&mut conflict, field.bytes()?)?,
            _ => return Err(JjViewDecodeError("unknown ref target field")),
        }
    }
    let bytes = conflict.ok_or(JjViewDecodeError("unsupported legacy empty ref target"))?;
    let mut fields = Fields::new(bytes);
    let mut removes = Vec::new();
    let mut adds = Vec::new();
    while let Some(field) = fields.next(&[(1, 2), (2, 2)])? {
        budgets.entry()?;
        let tag = field.tag;
        let value = term(field.bytes()?, budgets, semantic)?;
        match tag {
            1 => removes.push(value),
            2 => adds.push(value),
            _ => return Err(JjViewDecodeError("unknown conflict field")),
        }
    }
    if adds.len() != removes.len() + 1 {
        return Err(JjViewDecodeError("invalid conflict arity"));
    }
    let mut values = Vec::with_capacity(removes.len() + adds.len());
    let mut removes = removes.into_iter();
    for added in adds {
        values.push(added);
        if let Some(removed) = removes.next() {
            values.push(removed);
        }
    }
    Ok(values)
}

pub(super) fn term<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
    semantic: bool,
) -> Result<Option<&'a [u8]>, JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut value = None;
    while let Some(field) = fields.next(&[(1, 2)])? {
        singular(&mut value, identity(field.bytes()?, 20)?)?;
    }
    if semantic && value.is_some() {
        budgets.commit()?;
    }
    Ok(value)
}

pub(super) fn state(value: u64) -> Result<u32, JjViewDecodeError> {
    match value {
        0 | 1 => Ok(value as u32),
        _ => Err(JjViewDecodeError("unknown remote state")),
    }
}
