#!/usr/bin/env python3
"""Independent CBOR fixtures for Git AI registration record inspection.

Only Python's standard library is used. The wire schema is frozen in
docs/decisions/2026-09-08-jj-native-registration-records.md. This is a record-format
oracle, not a filesystem registration or native jj evidence producer.
"""

import argparse
import copy
import hashlib
import json
from pathlib import Path


PROFILE = "jj-simple-op-store/0.45.1"
BASELINE = "b78dbce79e7503edc15d8264431c6ccd851a68c67e84c0f390d499302c3f590a"
# These diagnostic IDs are structurally checked here, not joined to native
# evidence. The fixed view comes from the independently hashed minimal view.
OPERATION = (
    "63649fe7673c81de03def51e538a3b0cd13a786bde3780a3a463c6eb27ab3a3018"
    "ff3d270d67789b4d65069349e13051d122f108a025c78b9e6f887532efc203"
)
VIEW = (
    "89fe796cbea6137bb7e28873043c87621df15c272c04d8ad54bcfe559230740d811"
    "98e0fab63001a015cb647c1b1338a68d80a5c869c19f99f79c3a0a5ebe96c"
)
KIB = 1024
SOURCE_DOMAIN = "git-ai/jj/source-registration/v1"
WORKSPACE_DOMAIN = "git-ai/jj/workspace-attachment/v1"


class Raw:
    def __init__(self, data):
        self.data = data


class MapPairs:
    def __init__(self, pairs):
        self.pairs = pairs


def head(major, value):
    if value < 24:
        return bytes([(major << 5) | value])
    for width, marker in [(1, 24), (2, 25), (4, 26), (8, 27)]:
        if value < 1 << (width * 8):
            return bytes([(major << 5) | marker]) + value.to_bytes(width, "big")
    raise ValueError("CBOR integer exceeds u64")


def cbor(value):
    if isinstance(value, Raw):
        return value.data
    if isinstance(value, bool):
        return b"\xf5" if value else b"\xf4"
    if value is None:
        return b"\xf6"
    if isinstance(value, int):
        return head(0, value) if value >= 0 else head(1, -value - 1)
    if isinstance(value, bytes):
        return head(2, len(value)) + value
    if isinstance(value, str):
        data = value.encode("utf-8")
        return head(3, len(data)) + data
    if isinstance(value, (list, tuple)):
        return head(4, len(value)) + b"".join(cbor(item) for item in value)
    if isinstance(value, (dict, MapPairs)):
        pairs = list(value.items()) if isinstance(value, dict) else value.pairs
        return head(5, len(pairs)) + b"".join(cbor(k) + cbor(v) for k, v in pairs)
    raise TypeError(type(value))


def digest(data):
    return hashlib.sha256(data).hexdigest()


def identity(device, inode):
    return {"device": device, "inode": inode}


def root_key(domain, platform, directory):
    return cbor({
        "domain": domain,
        "platform": platform,
        "device": directory["device"],
        "inode": directory["inode"],
    })


def guard_preimages(kind, record):
    if kind == "source":
        binding = record["source_binding"]
        return {
            "source_root_key": root_key(
                "git-ai/jj/source-root-guard/v1",
                binding["platform"],
                binding["directories"][0],
            )
        }
    locator = record["locator"]
    return {
        "locator_key": cbor({
            "domain": "git-ai/jj/workspace-locator/v1",
            "platform": locator["platform"],
            "workspace_root": locator["workspace_root"],
        }),
        "workspace_root_key": root_key(
            "git-ai/jj/workspace-root-guard/v1",
            locator["platform"],
            record["workspace_binding"]["directories"][0],
        ),
    }


def workspace(platform="linux", source=1, name="default", number=201):
    return {
        "record_version": 1,
        "domain": WORKSPACE_DOMAIN,
        "source_id": f"{source:064x}",
        "reader_profile": PROFILE,
        "baseline_id": BASELINE,
        "baseline_generation": 1,
        "seal_digest": digest(b"fixture-seal\n"),
        "workspace_name": name,
        "attachment_id": f"{number:064x}",
        "locator": {
            "platform": platform,
            "workspace_root": f"/fixture/workspace-{number}".encode(),
        },
        "workspace_binding": {
            "directories": [identity(1, number + i) for i in range(4)],
            "colocated": False,
        },
        "selected_checkout": {
            "raw_checkout_bytes": b"opaque-checkout\x00\xff",
            "operation_id": OPERATION,
            "view_id": VIEW,
            "baseline_relation": "outside_baseline",
        },
    }


def source_record(selected, seal=b"fixture-seal\n"):
    platform = selected["locator"]["platform"]
    return {
        "record_version": 1,
        "domain": SOURCE_DOMAIN,
        "source_id": selected["source_id"],
        "reader_profile": PROFILE,
        "baseline_id": BASELINE,
        "baseline_generation": 1,
        "seal_bytes": seal,
        "seal_digest": digest(seal),
        "source_binding": {
            "platform": platform,
            "identity_format": "unix-device-inode/v1",
            "directories": [identity(1, 101 + i) for i in range(8)],
            "backends": [b"git\n", b"simple_op_store\n", b"simple_op_heads_store\n"],
        },
        "initial_workspace_name": selected["workspace_name"],
        "initial_attachment_id": selected["attachment_id"],
        "initial_workspace_record_id": digest(cbor(selected)),
    }


def canonical_vectors():
    records = {}
    for platform in ["linux", "macos"]:
        selected = workspace(platform)
        records[f"{platform}_source"] = ("source", source_record(selected))
        records[f"{platform}_workspace"] = ("workspace", selected)
    other = workspace(source=2, number=901)
    other_source = source_record(other)
    other_source["source_binding"]["directories"] = [identity(1, 1101 + i) for i in range(8)]
    records["other_source"] = ("source", other_source)
    records["other_workspace"] = ("workspace", other)
    for label, name, number in [
        ("case", "Default", 301),
        ("unicode", "workspace-工", 401),
        ("nfc", "é", 501),
        ("nfd", "e\u0301", 601),
    ]:
        records[f"{label}_workspace"] = ("workspace", workspace(name=name, number=number))
    large = workspace(name="w" * 5000, number=701)
    large["locator"]["workspace_root"] = b"/" + b"L" * 4998 + b"\xff"
    large["selected_checkout"]["raw_checkout_bytes"] = b"C" * 5000
    large_source = source_record(large)
    large_source["source_binding"]["backends"] = [b"B" * 5000] * 3
    records["large_source"] = ("source", large_source)
    records["large_workspace"] = ("workspace", large)
    maximum = workspace(name="é" * 8192, number=801)
    maximum["locator"]["workspace_root"] = b"/" + b"L" * (64 * KIB - 2) + b"\xff"
    maximum["selected_checkout"]["raw_checkout_bytes"] = b"\xff" * (16 * KIB)
    maximum["seal_digest"] = digest(b"S" * KIB)
    maximum["workspace_binding"]["directories"] = [identity(2**64 - 1, 2**64 - 1)] * 4
    maximum_source = source_record(maximum, b"S" * KIB)
    maximum_source["source_binding"]["directories"] = [identity(2**64 - 1, 2**64 - 1)] * 8
    maximum_source["source_binding"]["backends"] = [b"\xff" * (16 * KIB)] * 3
    records["max_source"] = ("source", maximum_source)
    records["max_workspace"] = ("workspace", maximum)
    zero = workspace(number=1001)
    zero["workspace_binding"]["directories"] = [identity(0, 0)] * 4
    zero["workspace_binding"]["colocated"] = True
    zero["selected_checkout"]["view_id"] = "0" * 128
    zero["selected_checkout"]["baseline_relation"] = "baseline_anchor"
    records["zero_identity_workspace"] = ("workspace", zero)
    return records


def changed(record, path, value):
    result = copy.deepcopy(record)
    target = result
    for part in path[:-1]:
        target = target[part]
    target[path[-1]] = value
    return result


def malformed_vectors(records):
    source = records["linux_source"][1]
    selected = records["linux_workspace"][1]
    vectors = []

    def add(label, kind, base, raw, reason, guards=None):
        vectors.append((label, kind, base, raw, reason, guards))

    def field(label, kind, path, value, reason):
        base = source if kind == "source" else selected
        mutant = changed(base, path, value)
        # Guard columns do not select a row. Recompute them for local-value
        # mutants so a stale guard does not hide a missing local validator.
        guards = guard_preimages(kind, mutant)
        if label == "array_locator":
            # If a permissive byte visitor accepts this array, its intended
            # bytes must still match the scalar guard. Canonical rejection is
            # the condition under test, not a representation-dependent key.
            guards = guard_preimages(kind, base)
        add(label, kind, base, cbor(mutant), reason, guards)

    add("reordered_source", "source", source, cbor(dict(reversed(list(source.items())))), "map order")
    locator = dict(reversed(list(selected["locator"].items())))
    field("reordered_locator", "workspace", ["locator"], locator, "nested map order")
    duplicate = list(source.items()) + [("record_version", 1)]
    add("duplicate_source", "source", source, cbor(MapPairs(duplicate)), "duplicate field")
    unknown = {**selected, "padding": 0}
    add("unknown_workspace", "workspace", selected, cbor(unknown), "unknown field")
    field("nonminimal_version", "source", ["record_version"], Raw(b"\x18\x01"), "nonminimal integer")
    for label, kind, path, value in [
        ("array_seal", "source", ["seal_bytes"], list(source["seal_bytes"])),
        ("array_backend", "source", ["source_binding", "backends", 0], [103, 105, 116, 10]),
        ("array_locator", "workspace", ["locator", "workspace_root"], list(selected["locator"]["workspace_root"])),
        ("array_checkout", "workspace", ["selected_checkout", "raw_checkout_bytes"], [1]),
    ]:
        field(label, kind, path, value, "byte string encoded as integer array")
    for label, kind, path, value, reason in [
        ("short_source_directories", "source", ["source_binding", "directories"], source["source_binding"]["directories"][:-1], "array length"),
        ("long_workspace_directories", "workspace", ["workspace_binding", "directories"], selected["workspace_binding"]["directories"] + [identity(1, 9)], "array length"),
        ("short_backends", "source", ["source_binding", "backends"], [b"a", b"b"], "array length"),
        ("wrong_platform", "source", ["source_binding", "platform"], "windows", "platform"),
        ("wrong_identity_format", "source", ["source_binding", "identity_format"], "unix-device-inode/v2", "identity format"),
        ("negative_device", "source", ["source_binding", "directories", 0, "device"], -1, "unsigned identity"),
        ("wrong_domain", "workspace", ["domain"], SOURCE_DOMAIN, "domain"),
        ("wrong_profile", "workspace", ["reader_profile"], PROFILE + "+unknown", "profile"),
        ("wrong_generation", "source", ["baseline_generation"], 2, "generation"),
        ("wrong_version", "source", ["record_version"], 2, "version"),
        ("seal_digest_mismatch", "source", ["seal_digest"], "f" * 64, "local seal digest"),
        ("root_operation", "workspace", ["selected_checkout", "operation_id"], "0" * 128, "virtual operation"),
        ("uppercase_view", "workspace", ["selected_checkout", "view_id"], "A" * 128, "lowercase ID"),
        ("wrong_relation", "workspace", ["selected_checkout", "baseline_relation"], "current", "relation"),
        ("relative_locator", "workspace", ["locator", "workspace_root"], b"relative", "rooted locator"),
        ("nul_locator", "workspace", ["locator", "workspace_root"], b"/a\x00b", "NUL locator"),
        ("empty_checkout", "workspace", ["selected_checkout", "raw_checkout_bytes"], b"", "nonempty checkout"),
        ("empty_name", "workspace", ["workspace_name"], "", "empty name plus original request-name mismatch"),
        ("empty_backend", "source", ["source_binding", "backends", 1], b"", "nonempty backend"),
    ]:
        field(label, kind, path, value, reason)
    empty_seal = changed(source, ["seal_bytes"], b"")
    empty_seal["seal_digest"] = digest(b"")
    add("empty_seal", "source", source, cbor(empty_seal), "nonempty seal with matching digest")
    # Each +1 changes only its named bounded field on a small otherwise valid
    # record. Seal digest is updated too so that its byte limit is isolated.
    over_seal = changed(source, ["seal_bytes"], b"S" * (KIB + 1))
    over_seal["seal_digest"] = digest(over_seal["seal_bytes"])
    add("over_seal", "source", source, cbor(over_seal), "seal bytes 1025")
    for label, kind, path, value, reason in [
        ("over_backend", "source", ["source_binding", "backends", 1], b"B" * (16 * KIB + 1), "backend bytes 16385"),
        ("over_initial_name", "source", ["initial_workspace_name"], "é" * 8192 + "x", "initial name UTF-8 bytes 16385"),
        ("over_workspace_name", "workspace", ["workspace_name"], "é" * 8192 + "x", "name UTF-8 bytes 16385 plus original request-name mismatch"),
        ("over_locator", "workspace", ["locator", "workspace_root"], b"/" + b"L" * (64 * KIB), "locator bytes 65537"),
        ("over_checkout", "workspace", ["selected_checkout", "raw_checkout_bytes"], b"C" * (16 * KIB + 1), "checkout bytes 16385"),
    ]:
        field(label, kind, path, value, reason)
    add("trailing_source", "source", source, cbor(source) + b"\x00", "trailing item")
    add("indefinite_source", "source", source, b"\xbf" + cbor(source)[1:] + b"\xff", "indefinite map")
    add("deep_source", "source", source, b"\x81" * 18 + b"\x00", "depth before DTO decode")
    for size, label in [(128 * KIB, "outer_exact"), (128 * KIB + 1, "outer_over")]:
        payload = b"\x00" * (size - 5)
        raw = cbor(payload)
        assert len(raw) == size
        add(label, "source", source, raw, "outer bytes; intentionally wrong root type")
    return vectors


def metadata(label, kind, base, raw, valid, reason, guards=None):
    preimages = guard_preimages(kind, base) if guards is None else guards
    result = {
        "label": label,
        "kind": kind,
        "source_id": base["source_id"],
        "baseline_id": base["baseline_id"],
        "workspace_name": base["workspace_name"] if kind == "workspace" else "",
        "source_root_key": "",
        "locator_key": "",
        "workspace_root_key": "",
        "checksum": digest(raw),
        "valid": valid,
        "bytes": len(raw),
        "reason": reason,
        "file": f"fixtures/{label}.cbor",
    }
    result.update({key: digest(value) for key, value in preimages.items()})
    if len(raw) <= 4096:
        result["hex"] = raw.hex()
    return result


def rust_string(value):
    return json.dumps(value, ensure_ascii=False)


def write_vectors(output, vectors):
    fields = [
        "label", "kind", "source_id", "baseline_id", "workspace_name",
        "source_root_key", "locator_key", "workspace_root_key", "checksum",
    ]
    lines = ["// Generated independently by generate_registration_records.py."]
    lines += ["pub(super) struct RecordVector {"]
    for field in fields:
        lines.append(f"    pub(super) {field}: &'static str,")
    lines += ["    pub(super) raw: &'static [u8],", "    pub(super) valid: bool,", "}", ""]
    lines.append("pub(super) const RECORDS: &[RecordVector] = &[")
    for vector in vectors:
        lines.append("    RecordVector {")
        for field in fields:
            value = vector[field]
            if field == "workspace_name" and len(value.encode()) > 128:
                name = f"{vector['label']}.name.txt"
                (output / "fixtures" / name).write_text(value, encoding="utf-8")
                rendered = f'include_str!("../fixtures/jj-registration-records/{name}")'
            else:
                rendered = rust_string(value)
            line = f"        {field}: {rendered},"
            if rendered.startswith("include_str!") and len(line) > 100:
                lines += [f"        {field}: include_str!(", f"            {rust_string('../fixtures/jj-registration-records/' + name)}", "        ),"]
            else:
                lines.append(line)
        lines.append(f'        raw: include_bytes!("../fixtures/jj-registration-records/{vector["label"]}.cbor"),')
        lines.append(f'        valid: {str(vector["valid"]).lower()},')
        lines.append("    },")
    lines.append("];")
    (output / "jj_registration_record_vectors.rs").write_text("\n".join(lines) + "\n")


def generate(output):
    (output / "fixtures").mkdir(parents=True, exist_ok=True)
    records = canonical_vectors()
    vectors = []
    for label, (kind, record) in records.items():
        raw = cbor(record)
        vectors.append(metadata(label, kind, record, raw, True, "canonical"))
        (output / "fixtures" / f"{label}.cbor").write_bytes(raw)
    for label, kind, base, raw, reason, guards in malformed_vectors(records):
        vectors.append(metadata(label, kind, base, raw, False, reason, guards))
        (output / "fixtures" / f"{label}.cbor").write_bytes(raw)
    write_vectors(output, vectors)
    guards = {}
    for label in ["linux_source", "linux_workspace", "macos_source", "macos_workspace"]:
        kind, record = records[label]
        guards[label] = {
            key: {"hex": raw.hex(), "sha256": digest(raw)}
            for key, raw in guard_preimages(kind, record).items()
        }
    report = {
        "schema": "git-ai/jj/registration-record-fixtures/v1",
        "profile": PROFILE,
        "vectors": vectors,
        "guard_preimages": guards,
        "valid_max_source_bytes": len(cbor(records["max_source"][1])),
        "valid_max_workspace_bytes": len(cbor(records["max_workspace"][1])),
        "note": "128KiB is an outer ceiling, unreachable by valid fixed-field records. Outside checkout bytes and IDs are historical, opaque data in this slice.",
    }
    (output / "manifest.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"vectors": len(vectors), "valid": len(records), "binary_bytes": sum(v["bytes"] for v in vectors), "source_max": report["valid_max_source_bytes"], "workspace_max": report["valid_max_workspace_bytes"]}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    generate(parser.parse_args().output_dir)
