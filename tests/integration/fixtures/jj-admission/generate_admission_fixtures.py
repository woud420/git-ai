#!/usr/bin/env python3
"""Independent canonical native-admission CBOR and fixed native-hash fixtures.

Packet/state maps follow the frozen git-ai admission/v1 contract. Native pairs
reuse independent jj v0.45.1 domain encoders (Apache-2.0 source attribution stays
in those generators). No Rust production writer or native decoder generates IDs.
"""
import argparse
import copy
import hashlib
import json
import sqlite3
import sys
from pathlib import Path

sys.dont_write_bytecode = True
PACKET_DOMAIN = "git-ai/jj/native-admission/packet/v1"
STATE_DOMAIN = "git-ai/jj/native-admission/state/v1"
LIMIT = 8 * 1024 * 1024


def digest(value):
    return hashlib.sha256(value).hexdigest()


def generate(oracle_dir, schema_v3, output):
    sys.path.insert(0, str(oracle_dir))
    from generate_registration_records import cbor
    from generate_evidence_vectors import pairs
    from generate_operation_vectors import metadata, operation

    conn = sqlite3.connect(":memory:")
    conn.executescript(schema_v3.read_text())
    source, baseline, receipt = conn.execute(
        "SELECT source_id,baseline_id,checksum FROM jj_native_registrations"
    ).fetchone()
    profile = "jj-simple-op-store/0.45.1"
    native = pairs()
    first = native["FIRST"][0][1]
    parents = {
        "FIRST": ["00" * 64], "LEFT": [first], "RIGHT": [first],
        "MERGE": [native["LEFT"][0][1], native["RIGHT"][0][1]],
        "UNRECORDED": ["00" * 64], "DETACHED": ["ee" * 64],
    }

    def evidence(name):
        (raw, identity), (view_raw, view_id) = native[name]
        return {
            "operation_id": identity, "parent_ids": parents[name],
            "view_id": view_id, "operation_bytes": raw, "view_bytes": view_raw,
        }

    def packet(expected, old_heads, heads, operations):
        return {
            "record_version": 1, "domain": PACKET_DOMAIN,
            "source_id": source, "reader_profile": profile,
            "initialization_receipt_id": receipt, "baseline_id": baseline,
            "baseline_generation": 1, "expected_admission_generation": expected,
            "expected_admitted_head_ids": sorted(old_heads),
            "captured_head_ids": sorted(heads), "operations": operations,
        }

    left, right, merge = (evidence(name) for name in ["LEFT", "RIGHT", "MERGE"])
    packet1 = packet(0, [first], [left["operation_id"]], [left])
    packet2 = packet(1, [left["operation_id"]], [merge["operation_id"]], [left, right, merge])
    packet3 = packet(2, [merge["operation_id"]], [left["operation_id"]], [left])

    def state(record, identity):
        return {
            "state_version": 1, "domain": STATE_DOMAIN,
            "source_id": source, "reader_profile": profile,
            "initialization_receipt_id": receipt, "baseline_id": baseline,
            "baseline_generation": 1, "admission_id": identity,
            "generation": record["expected_admission_generation"] + 1,
            "admitted_head_ids": record["captured_head_ids"],
        }

    rows = []

    def save(label, kind, raw, base, valid=True, native_valid=True):
        identity = digest(raw) if kind == "packet" else base["admission_id"]
        checksum = digest(raw)
        path = output / f"{label}.cbor"
        path.write_bytes(raw)
        rows.append({
            "label": label, "kind": kind, "file": path.name,
            "bytes": len(raw), "checksum": checksum, "admission_id": identity,
            "generation": base["expected_admission_generation"] + 1 if kind == "packet" else base["generation"],
            "source_id": source, "model_valid": valid, "native_valid": native_valid,
        })
        return identity

    canonical = {"left": packet1, "merge": packet2, "return_left": packet3,
                 "baseline_only": packet(0, [first], [first], [])}
    sorted_forks = sorted([left, right], key=lambda record: record["operation_id"])
    canonical["two_heads"] = packet(0, [first], [left["operation_id"], right["operation_id"]], sorted_forks)
    unrecorded = evidence("UNRECORDED")
    canonical["root_closed"] = packet(0, [first], [unrecorded["operation_id"]], [unrecorded])
    view_raw, view_id = native["FIRST"][1]
    large_raw, large_id = operation(view=bytes.fromhex(view_id), parents=[bytes.fromhex(first)],
                                    predecessors=[], meta=metadata(description=b"x" * 5000))
    large_evidence = {"operation_id": large_id, "parent_ids": [first], "view_id": view_id,
                      "operation_bytes": large_raw, "view_bytes": view_raw}
    canonical["large_bytes"] = packet(0, [first], [large_id], [large_evidence])
    for label, record in canonical.items():
        identity = save(label, "packet", cbor(record), record)
        current = state(record, identity)
        save(label + "_state", "state", cbor(current), current)

    wrong = copy.deepcopy(packet1)
    wrong["operations"][0]["operation_bytes"] = b"\xff" + left["operation_bytes"][1:]
    identity = save("wrong_native", "packet", cbor(wrong), wrong, native_valid=False)
    save("wrong_native_state", "state", cbor(state(wrong, identity)), state(wrong, identity), native_valid=False)
    wrong_order = copy.deepcopy(packet2)
    wrong_order["operations"] = list(reversed(wrong_order["operations"]))
    save("wrong_native_order", "packet", cbor(wrong_order), wrong_order, native_valid=False)
    parent_mismatch = copy.deepcopy(packet1)
    parent_mismatch["operations"][0]["parent_ids"] = [right["operation_id"]]
    save("wrong_native_parent", "packet", cbor(parent_mismatch), parent_mismatch, native_valid=False)

    def mutant(label, change):
        value = copy.deepcopy(packet1)
        change(value)
        save(label, "packet", cbor(value), packet1, valid=False, native_valid=False)

    mutant("wrong_domain", lambda value: value.update(domain="git-ai/jj/native-admission/packet/v2"))
    mutant("wrong_version", lambda value: value.update(record_version=2))
    mutant("unknown_field", lambda value: value.update(extra=0))
    mutant("wrong_source", lambda value: value.update(source_id="02" * 32))
    mutant("uppercase_operation", lambda value: value["operations"][0].update(operation_id=left["operation_id"].upper()))
    mutant("root_operation", lambda value: value["operations"][0].update(operation_id="00" * 64))
    mutant("array_operation_bytes", lambda value: value["operations"][0].update(operation_bytes=list(left["operation_bytes"])))
    mutant("empty_operation_bytes", lambda value: value["operations"][0].update(operation_bytes=b""))
    mutant("duplicate_operation", lambda value: value["operations"].append(copy.deepcopy(value["operations"][0])))
    mutant("duplicate_head", lambda value: value["captured_head_ids"].append(left["operation_id"]))
    mutant("zero_parent_count", lambda value: value["operations"][0].update(parent_ids=[]))
    mutant("parents_33", lambda value: value["operations"][0].update(parent_ids=[f"{i:0128x}" for i in range(33)]))
    encoded = cbor(packet1)
    assert encoded[0] == 0xab
    reordered = dict(reversed(list(packet1.items())))
    save("reordered_map", "packet", cbor(reordered), packet1, valid=False, native_valid=False)
    key = cbor("record_version")
    assert encoded[1:1 + len(key)] == key and encoded[1 + len(key)] == 1
    save("nonminimal_version", "packet", encoded[:1 + len(key)] + b"\x18\x01" + encoded[2 + len(key):], packet1, valid=False, native_valid=False)
    save("duplicate_field", "packet", b"\xac" + encoded[1:] + key + b"\x01", packet1, valid=False, native_valid=False)
    save("indefinite_map", "packet", b"\xbf" + encoded[1:] + b"\xff", packet1, valid=False, native_valid=False)
    save("tagged_map", "packet", b"\xc0" + encoded, packet1, valid=False, native_valid=False)
    save("trailing_bytes", "packet", encoded + b"\x00", packet1, valid=False, native_valid=False)

    def chain(description_size, last_extra=0):
        records = []
        previous = first
        for index in range(256):
            size = description_size + (last_extra if index == 255 else 0)
            raw, identity = operation(view=bytes.fromhex(view_id), parents=[bytes.fromhex(previous)],
                                      predecessors=[], meta=metadata(description=b"x" * size))
            records.append({"operation_id": identity, "parent_ids": [previous], "view_id": view_id,
                            "operation_bytes": raw, "view_bytes": view_raw})
            previous = identity
        return packet(0, [first], [previous], records)

    low, high = 0, 64 * 1024
    while low < high:
        middle = (low + high + 1) // 2
        if len(cbor(chain(middle))) <= LIMIT:
            low = middle
        else:
            high = middle - 1
    uniform = chain(low)
    remainder = LIMIT - len(cbor(uniform))
    exact = chain(low, remainder)
    over = chain(low, remainder + 1)
    assert len(cbor(exact)) == LIMIT and len(cbor(over)) == LIMIT + 1
    for record in [exact, over]:
        assert len(record["operations"]) == 256
        assert sum(len(item["operation_bytes"]) + len(item["view_bytes"]) for item in record["operations"]) < LIMIT
        assert max(len(item["operation_bytes"]) + len(item["view_bytes"]) for item in record["operations"]) < 1024 * 1024
    boundary = {
        "description_size": low, "last_extra": remainder,
        "operation_ids": [item["operation_id"] for item in exact["operations"]],
        "over_last_operation_id": over["captured_head_ids"][0],
        "exact_bytes": len(cbor(exact)), "over_bytes": len(cbor(over)),
        "exact_admission_id": digest(cbor(exact)), "over_admission_id": digest(cbor(over)),
        "raw_bytes": sum(len(item["operation_bytes"]) + len(item["view_bytes"]) for item in exact["operations"]),
        "view_id": view_id, "view_hex": view_raw.hex(),
    }
    (output / "boundary.json").write_text(json.dumps(boundary, indent=2) + "\n")
    metadata_out = {"source_id": source, "baseline_id": baseline, "initialization_receipt_id": receipt,
                    "reader_profile": profile, "first_id": first, "records": rows,
                    "boundary": {key: value for key, value in boundary.items() if key != "operation_ids"},
                    "schema_v3_sha256": digest(schema_v3.read_bytes())}
    (output / "metadata.json").write_text(json.dumps(metadata_out, indent=2) + "\n")
    rust = ["// Generated independently by generate_admission_fixtures.py.",
            "pub struct RecordVector {", "    pub label: &'static str,", "    pub kind: &'static str,",
            "    pub raw: &'static [u8],", "    pub admission_id: &'static str,",
            "    pub checksum: &'static str,", "    pub generation: u64,",
            "    pub model_valid: bool,", "    pub native_valid: bool,", "}"]
    for name, value in [("SOURCE", source), ("BASELINE", baseline), ("RECEIPT", receipt),
                        ("PROFILE", profile), ("FIRST", first)]:
        rust.append(f'pub const {name}: &str = "{value}";')
    rust.append("pub const RECORDS: &[RecordVector] = &[")
    for row in rows:
        rust.extend(["    RecordVector {", f'        label: "{row["label"]}",',
                     f'        kind: "{row["kind"]}",',
                     f'        raw: include_bytes!("{row["file"]}"),',
                     f'        admission_id: "{row["admission_id"]}",',
                     f'        checksum: "{row["checksum"]}",',
                     f'        generation: {row["generation"]},',
                     f'        model_valid: {str(row["model_valid"]).lower()},',
                     f'        native_valid: {str(row["native_valid"]).lower()},', "    },"])
    rust.append("];\n")
    (output / "vectors.rs").write_text("\n".join(rust))
    boundary_rust = ["// Generated native identity expectations; raw bytes are reconstructed in tests."]
    for name, value in [("DESCRIPTION_BYTES", low), ("LAST_EXTRA_BYTES", remainder),
                        ("RAW_BYTES", boundary["raw_bytes"])]:
        boundary_rust.append(f"pub const {name}: usize = {value};")
    for name, key in [("EXACT_ADMISSION_ID", "exact_admission_id"), ("OVER_ADMISSION_ID", "over_admission_id"),
                      ("OVER_LAST_OPERATION_ID", "over_last_operation_id"), ("VIEW_ID", "view_id"), ("VIEW_HEX", "view_hex")]:
        boundary_rust.append(f'pub const {name}: &str = "{boundary[key]}";')
    boundary_rust.append("pub const OPERATION_IDS: &[&str] = &[")
    boundary_rust.extend(f'    "{identity}",' for identity in boundary["operation_ids"])
    boundary_rust.append("];\n")
    (output / "boundary.rs").write_text("\n".join(boundary_rust))
    print(json.dumps({"records": len(rows), "total_binary_bytes": sum(row["bytes"] for row in rows),
                      "boundary": {key: boundary[key] for key in ["description_size", "last_extra", "exact_bytes", "raw_bytes"]}}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-dir", required=True, type=Path)
    parser.add_argument("--schema-v3", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    generate(args.oracle_dir, args.schema_v3, args.output_dir)
