//! Isolated codec measurements and diagnostic control-socket measurements.
//! See `scripts/benchmarks/checkpoint/IPC.md` for the measurement contracts.

mod codecs;
mod fixtures;
mod report;
mod socket;

use codecs::Codec;
use fixtures::FIXTURES;
use report::{BenchSettings, Measurement, measure_batch, write_report};
use serde_json::{Value, json};
use std::hint::black_box;
use std::time::Instant;

#[test]
fn checkpoint_ipc_benchmark_fixtures_roundtrip_all_fields() {
    let root = std::env::temp_dir().join("git-ai-ipc-codec-fixture");
    for fixture in FIXTURES {
        let delivery = fixture.delivery(&root, 7);
        delivery.validate().unwrap();
        for codec in Codec::ALL {
            let encoded = codec.encode(&delivery);
            let decoded = codec.decode(&encoded);
            assert_eq!(
                serde_json::to_value(&decoded).unwrap(),
                serde_json::to_value(&delivery).unwrap(),
                "{} / {} must preserve every delivery field",
                fixture.name,
                codec.name()
            );
        }
    }
}

#[test]
#[ignore = "explicit codec performance run; emits a machine-readable report"]
fn benchmark_checkpoint_ipc_codecs() {
    let settings = BenchSettings::from_env();
    let root = std::env::temp_dir().join("git-ai-ipc-codec-fixture");
    let mut results = Vec::new();

    for fixture in FIXTURES {
        let delivery = fixture.delivery(&root, 7);
        let encoded: Vec<_> = Codec::ALL
            .iter()
            .map(|codec| codec.encode(&delivery))
            .collect();
        let mut timings: Vec<_> = Codec::ALL
            .iter()
            .map(|_| [Vec::new(), Vec::new(), Vec::new()])
            .collect();
        for (codec, bytes) in Codec::ALL.iter().zip(&encoded) {
            let until = Instant::now() + settings.codec_warmup;
            while Instant::now() < until {
                black_box(codec.decode(&codec.encode(black_box(&delivery))));
                black_box(codec.decode(black_box(bytes)));
            }
        }
        for sample in 0..settings.samples {
            // Alternate both orders so a codec or operation is not always last.
            for offset in 0..Codec::ALL.len() {
                let index = if sample % 2 == 0 {
                    offset
                } else {
                    Codec::ALL.len() - 1 - offset
                };
                let codec = Codec::ALL[index];
                for operation_offset in 0..3 {
                    let operation = (sample + operation_offset) % 3;
                    let measured = match operation {
                        0 => measure_batch(settings.sample_time, || {
                            black_box(codec.encode(black_box(&delivery)));
                        }),
                        1 => measure_batch(settings.sample_time, || {
                            black_box(codec.decode(black_box(&encoded[index])));
                        }),
                        _ => measure_batch(settings.sample_time, || {
                            black_box(codec.decode(&codec.encode(black_box(&delivery))));
                        }),
                    };
                    timings[index][operation].push(measured);
                }
            }
        }
        for (index, codec) in Codec::ALL.iter().enumerate() {
            results.push(json!({
                "fixture": fixture.name,
                "files": fixture.files,
                "content_bytes": fixture.content_bytes(),
                "codec": codec.name(),
                "encoded_bytes": encoded[index].len(),
                "encode": Measurement::from_batches(&timings[index][0]),
                "decode_owned": Measurement::from_batches(&timings[index][1]),
                "roundtrip_owned": Measurement::from_batches(&timings[index][2]),
            }));
        }
        let clones: Vec<_> = (0..settings.samples)
            .map(|_| {
                measure_batch(settings.sample_time, || {
                    black_box(black_box(&delivery).clone());
                })
            })
            .collect();
        results.push(json!({
            "fixture": fixture.name,
            "files": fixture.files,
            "content_bytes": fixture.content_bytes(),
            "operation": "delivery_clone",
            "measurement": Measurement::from_batches(&clones),
        }));
    }
    write_report("codec", &settings, Value::Array(results), None);
}
