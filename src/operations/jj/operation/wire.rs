use super::JjDecodeError;

pub(super) struct Fields<'a> {
    remaining: &'a [u8],
}

pub(super) struct Field<'a> {
    pub tag: u64,
    value: Value<'a>,
}

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

impl<'a> Fields<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    pub fn next(&mut self, allowed: &[(u64, u8)]) -> Result<Option<Field<'a>>, JjDecodeError> {
        if self.remaining.is_empty() {
            return Ok(None);
        }
        let key = self.varint()?;
        let tag = key >> 3;
        let wire_type = (key & 7) as u8;
        if !allowed.contains(&(tag, wire_type)) {
            return Err(JjDecodeError("unknown field or wrong wire type"));
        }
        let value = match wire_type {
            0 => Value::Varint(self.varint()?),
            2 => {
                let length = usize::try_from(self.varint()?)
                    .map_err(|_| JjDecodeError("truncated field length"))?;
                if length > self.remaining.len() {
                    return Err(JjDecodeError("truncated field"));
                }
                let (value, remaining) = self.remaining.split_at(length);
                self.remaining = remaining;
                Value::Bytes(value)
            }
            _ => return Err(JjDecodeError("unsupported field wire type")),
        };
        Ok(Some(Field { tag, value }))
    }

    fn varint(&mut self) -> Result<u64, JjDecodeError> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let Some((&byte, remaining)) = self.remaining.split_first() else {
                return Err(JjDecodeError("truncated varint"));
            };
            self.remaining = remaining;
            if shift == 63 && byte > 1 {
                return Err(JjDecodeError("varint overflow"));
            }
            value |= u64::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(JjDecodeError("varint overflow"))
    }
}

impl<'a> Field<'a> {
    pub fn bytes(self) -> Result<&'a [u8], JjDecodeError> {
        match self.value {
            Value::Bytes(value) => Ok(value),
            Value::Varint(_) => Err(JjDecodeError("wrong field wire type")),
        }
    }

    pub fn varint(self) -> Result<u64, JjDecodeError> {
        match self.value {
            Value::Varint(value) => Ok(value),
            Value::Bytes(_) => Err(JjDecodeError("wrong field wire type")),
        }
    }
}

pub(super) fn singular<T>(slot: &mut Option<T>, value: T) -> Result<(), JjDecodeError> {
    if slot.is_some() {
        return Err(JjDecodeError("duplicate singular field"));
    }
    *slot = Some(value);
    Ok(())
}

pub(super) fn boolean(value: u64) -> Result<bool, JjDecodeError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(JjDecodeError("invalid bool")),
    }
}

pub(super) fn identity(bytes: &[u8], length: usize) -> Result<&[u8], JjDecodeError> {
    if bytes.len() != length {
        return Err(JjDecodeError("invalid identity length"));
    }
    Ok(bytes)
}

pub(super) struct Budget {
    remaining: usize,
    error: &'static str,
}

impl Budget {
    pub fn new(limit: usize, error: &'static str) -> Self {
        Self {
            remaining: limit,
            error,
        }
    }

    pub fn take(&mut self, count: usize) -> Result<(), JjDecodeError> {
        self.remaining = self
            .remaining
            .checked_sub(count)
            .ok_or(JjDecodeError(self.error))?;
        Ok(())
    }
}
