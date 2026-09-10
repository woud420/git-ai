use crate::model::jj_observation::{
    JjOperationEvidence, MAX_JJ_OBSERVATION_OPERATION_BYTES, MAX_JJ_OBSERVATION_PARENTS,
};
use serde::de::{Error, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::marker::PhantomData;

pub(in crate::model::repository::jj_observation_journal) fn sequence<'de, D, T>(
    deserializer: D,
    limit: usize,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Limited<T> {
        limit: usize,
        element: PhantomData<T>,
    }
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Limited<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded native baseline sequence")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Vec<T>, A::Error> {
            let length = sequence
                .size_hint()
                .ok_or_else(|| A::Error::custom("missing sequence length"))?;
            if length > self.limit {
                return Err(A::Error::custom("native baseline sequence limit exceeded"));
            }
            let mut values = Vec::with_capacity(length);
            for _ in 0..length {
                values.push(
                    sequence
                        .next_element()?
                        .ok_or_else(|| A::Error::custom("short sequence"))?,
                );
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(A::Error::custom("sequence length mismatch"));
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Limited {
        limit,
        element: PhantomData,
    })
}

pub(super) fn heads<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    sequence(deserializer, super::types::MAX_HEADS)
}

fn parents<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    sequence(deserializer, MAX_JJ_OBSERVATION_PARENTS)
}

fn bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    sequence(deserializer, MAX_JJ_OBSERVATION_OPERATION_BYTES)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    operation_id: String,
    #[serde(deserialize_with = "parents")]
    parent_ids: Vec<String>,
    view_id: String,
    #[serde(deserialize_with = "bytes")]
    operation_bytes: Vec<u8>,
    #[serde(deserialize_with = "bytes")]
    view_bytes: Vec<u8>,
}

pub(super) fn anchors<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<JjOperationEvidence>, D::Error> {
    let records: Vec<Evidence> = sequence(deserializer, super::types::MAX_HEADS)?;
    Ok(records
        .into_iter()
        .map(|record| JjOperationEvidence {
            operation_id: record.operation_id,
            parent_ids: record.parent_ids,
            view_id: record.view_id,
            operation_bytes: record.operation_bytes,
            view_bytes: record.view_bytes,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::{DeserializeSeed, value::Error, value::SeqAccessDeserializer};

    struct Unreadable {
        hint: Option<usize>,
    }

    impl<'de> SeqAccess<'de> for Unreadable {
        type Error = Error;

        fn next_element_seed<T: DeserializeSeed<'de>>(
            &mut self,
            _seed: T,
        ) -> Result<Option<T::Value>, Self::Error> {
            panic!("rejected sequence must not consume or deserialize an element");
        }

        fn size_hint(&self) -> Option<usize> {
            self.hint
        }
    }

    #[test]
    fn native_baseline_oversized_sequence_hints_reject_before_first_element() {
        for limit in [
            super::super::types::MAX_HEADS,
            MAX_JJ_OBSERVATION_PARENTS,
            MAX_JJ_OBSERVATION_OPERATION_BYTES,
            crate::model::jj_observation::MAX_JJ_OBSERVATION_OPERATIONS,
        ] {
            let source = SeqAccessDeserializer::new(Unreadable {
                hint: Some(limit + 1),
            });
            assert!(
                sequence::<_, u8>(source, limit)
                    .unwrap_err()
                    .to_string()
                    .contains("limit")
            );
        }
    }

    #[test]
    fn native_baseline_unknown_sequence_length_rejects_before_first_element() {
        let source = SeqAccessDeserializer::new(Unreadable { hint: None });
        assert!(
            sequence::<_, u8>(source, 32)
                .unwrap_err()
                .to_string()
                .contains("length")
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_admission_field_visitors_reuse_early_sequence_hint_guards() {
        use super::super::super::native_admission::bounded as admission;
        for (field, limit) in [("heads", 32), ("parents", 32), ("operations", 256)] {
            for hint in [None, Some(limit + 1)] {
                let source = SeqAccessDeserializer::new(Unreadable { hint });
                let error = match field {
                    "heads" => admission::heads(source).err(),
                    "parents" => admission::parents(source).err(),
                    _ => admission::operations(source).err(),
                }
                .unwrap();
                assert!(error.to_string().contains(if hint.is_none() {
                    "length"
                } else {
                    "limit"
                }));
            }
        }
    }
}
