#!/usr/bin/env python3
"""Freeze v2 independently of Rust journal/schema writers; preserve v1 verbatim.

The existing paired native oracle supplies FIRST's operation/view bytes and
identities. This small definite CBOR encoder mirrors the frozen v2 storage field
order from native_baseline/types.rs, including Vec<u8> encoded as integer arrays.
No runtime Python dependency or new native oracle is introduced.
"""

import argparse
import hashlib
import sys
from pathlib import Path

sys.dont_write_bytecode = True


def encode(value):
    def head(major, number):
        if number < 24:
            return bytes([major << 5 | number])
        for width, info in [(1, 24), (2, 25), (4, 26), (8, 27)]:
            if number < 1 << (width * 8):
                return bytes([major << 5 | info]) + number.to_bytes(width, 'big')
        raise ValueError('integer too large')

    if isinstance(value, int):
        return head(0, value)
    if isinstance(value, str):
        raw = value.encode('utf-8')
        return head(3, len(raw)) + raw
    if isinstance(value, (list, bytes)):
        return head(4, len(value)) + b''.join(encode(item) for item in value)
    if isinstance(value, dict):
        return head(5, len(value)) + b''.join(encode(key) + encode(item) for key, item in value.items())
    raise TypeError(type(value))


def fixture(oracle_dir, frozen_v1):
    sys.path.insert(0, str(oracle_dir))
    from generate_evidence_vectors import pairs
    operation, view = pairs()['FIRST']
    source = f'{1:064x}'
    profile = 'jj-simple-op-store/0.45.1'
    evidence = dict(operation_id=operation[1], parent_ids=['00' * 64],
                    view_id=view[1], operation_bytes=operation[0], view_bytes=view[0])
    record = dict(record_version=1, domain='git-ai/jj/current-state-baseline/install/v1',
                  source_id=source, reader_profile=profile, mode='current_state',
                  expected_native_generation=0, captured_head_ids=[operation[1]], anchors=[evidence])
    raw_record = encode(record)
    baseline_id = hashlib.sha256(raw_record).hexdigest()
    state = dict(state_version=1, source_id=source, reader_profile=profile,
                 baseline_id=baseline_id, generation=1, captured_head_ids=[operation[1]])
    raw_state = encode(state)
    suffix = f'''
-- Frozen v2 extension: native FIRST baseline is intentionally unregistered.
-- Generated with independent CBOR and the established jj 0.45.1 paired oracle.
CREATE TABLE jj_native_baselines (
    source_id TEXT NOT NULL,
    baseline_id TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, baseline_id)
);
CREATE TABLE jj_native_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);
UPDATE schema_metadata SET value = '2' WHERE key = 'version';
INSERT INTO jj_native_baselines(source_id, baseline_id, record, checksum)
VALUES ('{source}', '{baseline_id}', X'{raw_record.hex()}', '{baseline_id}');
INSERT INTO jj_native_sources(source_id, baseline_id, state, checksum)
VALUES ('{source}', '{baseline_id}', X'{raw_state.hex()}', '{hashlib.sha256(raw_state).hexdigest()}');
'''
    return frozen_v1 + suffix


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--oracle-dir', type=Path, required=True)
    parser.add_argument('--v1', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    original = args.v1.read_text()
    result = fixture(args.oracle_dir, original)
    assert result.startswith(original)
    args.output.write_text(result)
