use super::*;

#[test]
fn test_byte_offset_watermark_serialize() {
    let wm = ByteOffsetWatermark::new(1234);
    assert_eq!(wm.serialize(), "1234");
}

#[test]
fn test_byte_offset_watermark_deserialize() {
    let wm = ByteOffsetWatermark::from_str("5678").unwrap();
    assert_eq!(wm.0, 5678);
}

#[test]
fn test_byte_offset_watermark_advance() {
    let mut wm = ByteOffsetWatermark::new(100);
    wm.advance(50, 10);
    assert_eq!(wm.0, 150);
}

#[test]
fn test_byte_offset_watermark_roundtrip() {
    let original = ByteOffsetWatermark::new(9999);
    let serialized = original.serialize();
    let deserialized = ByteOffsetWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_byte_offset_watermark_invalid() {
    let result = ByteOffsetWatermark::from_str("not_a_number");
    assert!(result.is_err());
}

#[test]
fn test_record_index_watermark_serialize() {
    let wm = RecordIndexWatermark::new(42);
    assert_eq!(wm.serialize(), "42");
}

#[test]
fn test_record_index_watermark_deserialize() {
    let wm = RecordIndexWatermark::from_str("123").unwrap();
    assert_eq!(wm.0, 123);
}

#[test]
fn test_record_index_watermark_advance() {
    let mut wm = RecordIndexWatermark::new(10);
    wm.advance(1000, 5);
    assert_eq!(wm.0, 15);
}

#[test]
fn test_record_index_watermark_roundtrip() {
    let original = RecordIndexWatermark::new(7777);
    let serialized = original.serialize();
    let deserialized = RecordIndexWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_timestamp_watermark_serialize() {
    let ts = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let wm = TimestampWatermark::new(ts);
    assert_eq!(wm.serialize(), "2024-01-01T12:00:00+00:00");
}

#[test]
fn test_timestamp_watermark_deserialize() {
    let wm = TimestampWatermark::from_str("2024-01-01T12:00:00Z").unwrap();
    let expected = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(wm.0, expected);
}

#[test]
fn test_timestamp_watermark_advance_noop() {
    let ts = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut wm = TimestampWatermark::new(ts);
    let original_ts = wm.0;
    wm.advance(100, 10);
    assert_eq!(wm.0, original_ts); // Should not change
}

#[test]
fn test_timestamp_watermark_roundtrip() {
    let ts = DateTime::parse_from_rfc3339("2024-06-15T08:30:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let original = TimestampWatermark::new(ts);
    let serialized = original.serialize();
    let deserialized = TimestampWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_hybrid_watermark_serialize_with_timestamp() {
    let ts = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let wm = HybridWatermark::new(1000, 50, Some(ts));
    assert_eq!(wm.serialize(), "1000|50|2024-01-01T12:00:00+00:00");
}

#[test]
fn test_hybrid_watermark_serialize_without_timestamp() {
    let wm = HybridWatermark::new(2000, 100, None);
    assert_eq!(wm.serialize(), "2000|100|");
}

#[test]
fn test_hybrid_watermark_deserialize_with_timestamp() {
    let wm = HybridWatermark::from_str("1500|75|2024-01-01T12:00:00Z").unwrap();
    assert_eq!(wm.offset, 1500);
    assert_eq!(wm.record, 75);
    assert!(wm.timestamp.is_some());
}

#[test]
fn test_hybrid_watermark_deserialize_without_timestamp() {
    let wm = HybridWatermark::from_str("3000|150|").unwrap();
    assert_eq!(wm.offset, 3000);
    assert_eq!(wm.record, 150);
    assert!(wm.timestamp.is_none());
}

#[test]
fn test_hybrid_watermark_advance() {
    let mut wm = HybridWatermark::new(100, 10, None);
    wm.advance(50, 5);
    assert_eq!(wm.offset, 150);
    assert_eq!(wm.record, 15);
}

#[test]
fn test_hybrid_watermark_roundtrip_with_timestamp() {
    let ts = DateTime::parse_from_rfc3339("2024-03-15T10:30:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let original = HybridWatermark::new(5000, 250, Some(ts));
    let serialized = original.serialize();
    let deserialized = HybridWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_hybrid_watermark_roundtrip_without_timestamp() {
    let original = HybridWatermark::new(6000, 300, None);
    let serialized = original.serialize();
    let deserialized = HybridWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_hybrid_watermark_invalid_format() {
    let result = HybridWatermark::from_str("1000|50");
    assert!(result.is_err());
}

#[test]
fn test_hybrid_watermark_invalid_offset() {
    let result = HybridWatermark::from_str("abc|50|");
    assert!(result.is_err());
}

#[test]
fn test_hybrid_watermark_invalid_record() {
    let result = HybridWatermark::from_str("1000|xyz|");
    assert!(result.is_err());
}

#[test]
fn test_watermark_type_deserialize_byte_offset() {
    let wm = WatermarkType::ByteOffset.deserialize("1234").unwrap();
    assert_eq!(wm.serialize(), "1234");
}

#[test]
fn test_watermark_type_deserialize_record_index() {
    let wm = WatermarkType::RecordIndex.deserialize("42").unwrap();
    assert_eq!(wm.serialize(), "42");
}

#[test]
fn test_watermark_type_deserialize_timestamp() {
    let wm = WatermarkType::Timestamp
        .deserialize("2024-01-01T12:00:00Z")
        .unwrap();
    assert_eq!(wm.serialize(), "2024-01-01T12:00:00+00:00");
}

#[test]
fn test_watermark_type_deserialize_hybrid() {
    let wm = WatermarkType::Hybrid.deserialize("1000|50|").unwrap();
    assert_eq!(wm.serialize(), "1000|50|");
}

#[test]
fn test_watermark_type_deserialize_invalid() {
    let result = WatermarkType::ByteOffset.deserialize("invalid");
    assert!(result.is_err());
}

#[test]
fn test_watermark_type_display() {
    assert_eq!(WatermarkType::ByteOffset.to_string(), "ByteOffset");
    assert_eq!(WatermarkType::RecordIndex.to_string(), "RecordIndex");
    assert_eq!(WatermarkType::Timestamp.to_string(), "Timestamp");
    assert_eq!(WatermarkType::Hybrid.to_string(), "Hybrid");
    assert_eq!(
        WatermarkType::TimestampCursor.to_string(),
        "TimestampCursor"
    );
}

#[test]
fn test_watermark_type_from_str() {
    assert_eq!(
        WatermarkType::from_str("ByteOffset").unwrap(),
        WatermarkType::ByteOffset
    );
    assert_eq!(
        WatermarkType::from_str("RecordIndex").unwrap(),
        WatermarkType::RecordIndex
    );
    assert_eq!(
        WatermarkType::from_str("Timestamp").unwrap(),
        WatermarkType::Timestamp
    );
    assert_eq!(
        WatermarkType::from_str("Hybrid").unwrap(),
        WatermarkType::Hybrid
    );
    assert_eq!(
        WatermarkType::from_str("TimestampCursor").unwrap(),
        WatermarkType::TimestampCursor
    );
}

#[test]
fn test_watermark_type_from_str_invalid() {
    let result = WatermarkType::from_str("Invalid");
    assert!(result.is_err());
    match result {
        Err(StreamError::Parse { message, .. }) => {
            assert!(message.contains("Invalid watermark type"));
        }
        _ => panic!("Expected Parse error"),
    }
}

#[test]
fn test_watermark_type_roundtrip() {
    let types = [
        WatermarkType::ByteOffset,
        WatermarkType::RecordIndex,
        WatermarkType::Timestamp,
        WatermarkType::Hybrid,
        WatermarkType::TimestampCursor,
    ];

    for wm_type in &types {
        let serialized = wm_type.to_string();
        let deserialized = WatermarkType::from_str(&serialized).unwrap();
        assert_eq!(*wm_type, deserialized);
    }
}

#[test]
fn test_timestamp_cursor_watermark_serialize() {
    let wm = TimestampCursorWatermark::new(12345.0, "span_abc".to_string());
    assert_eq!(wm.serialize(), "12345|span_abc");
}

#[test]
fn test_timestamp_cursor_watermark_serialize_fractional() {
    let wm = TimestampCursorWatermark::new(12345.67, "span_abc".to_string());
    assert_eq!(wm.serialize(), "12345.67|span_abc");
}

#[test]
fn test_timestamp_cursor_watermark_deserialize() {
    let wm = TimestampCursorWatermark::from_str("67890|span_xyz").unwrap();
    assert_eq!(wm.timestamp_millis, 67890.0);
    assert_eq!(wm.last_id, "span_xyz");
}

#[test]
fn test_timestamp_cursor_watermark_deserialize_fractional() {
    let wm = TimestampCursorWatermark::from_str("67890.35|span_xyz").unwrap();
    assert_eq!(wm.timestamp_millis, 67890.35);
    assert_eq!(wm.last_id, "span_xyz");
}

#[test]
fn test_timestamp_cursor_watermark_initial() {
    let wm = TimestampCursorWatermark::initial();
    assert_eq!(wm.timestamp_millis, 0.0);
    assert_eq!(wm.last_id, "");
    assert_eq!(wm.serialize(), "0|");
}

#[test]
fn test_timestamp_cursor_watermark_roundtrip() {
    let original = TimestampCursorWatermark::new(999999.0, "my-span-id".to_string());
    let serialized = original.serialize();
    let deserialized = TimestampCursorWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_timestamp_cursor_watermark_roundtrip_fractional() {
    let original = TimestampCursorWatermark::new(1780519329188.35, "span_id".to_string());
    let serialized = original.serialize();
    let deserialized = TimestampCursorWatermark::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_timestamp_cursor_watermark_invalid_format() {
    let result = TimestampCursorWatermark::from_str("no_pipe_separator");
    assert!(result.is_err());
}

#[test]
fn test_timestamp_cursor_watermark_invalid_millis() {
    let result = TimestampCursorWatermark::from_str("not_a_number|span1");
    assert!(result.is_err());
}

#[test]
fn test_watermark_type_deserialize_timestamp_cursor() {
    let wm = WatermarkType::TimestampCursor
        .deserialize("5000|span_42")
        .unwrap();
    assert_eq!(wm.serialize(), "5000|span_42");
}
