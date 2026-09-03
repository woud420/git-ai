"""Authenticate and normalize checkpoint journal records for benchmark oracles.

Fresh benchmark runs authenticate the terminal-checksum v1 form emitted by the
writer. Rust's canonical/nonterminal v1 read-compatibility path is intentionally
outside this fresh-run oracle and remains covered by the storage recovery tests.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


V1_VERSION_FIELD = "_git_ai_record_version"
V1_CHECKSUM_FIELD = "_git_ai_record_checksum"
CHECKPOINT_API_VERSION = "checkpoint/1.0.0"
CHECKPOINT_KINDS = ("Human", "AiAgent", "AiTab", "KnownHuman")
V2_ALLOWED_FIELDS = frozenset(
    {"a", "d", "e", "g", "h", "i", "k", "m", "r", "s", "t", "v", "y", "c"}
)
V2_REQUIRED_FIELDS = frozenset({"a", "d", "e", "k", "s", "t", "v", "c"})
U8_MAX = 2**8 - 1
U32_MAX = 2**32 - 1
U64_MAX = 2**64 - 1
U128_MAX = 2**128 - 1
USIZE_MAX = sys.maxsize * 2 + 1


def _strict_json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def _reject_non_finite(value: str) -> None:
    raise ValueError(f"non-finite JSON number {value}")


def _parse_checkpoint_record(line: str) -> dict[str, Any]:
    try:
        record = json.loads(
            line,
            object_pairs_hook=_strict_json_object,
            parse_constant=_reject_non_finite,
        )
    except (json.JSONDecodeError, ValueError) as error:
        raise RuntimeError(f"malformed checkpoints.jsonl: {error}") from error
    if not isinstance(record, dict):
        raise RuntimeError("malformed checkpoints.jsonl: record is not an object")
    return record


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _verify_terminal_checksum(
    line: str, record: dict[str, Any], checksum_field: str
) -> None:
    marker = re.search(
        rf',"{re.escape(checksum_field)}":"([0-9a-f]{{64}})"\}}$', line
    )
    if marker is None:
        raise RuntimeError(
            f"checkpoint {checksum_field} checksum is missing or not terminal"
        )
    supplied = marker.group(1)
    if record.get(checksum_field) != supplied:
        raise RuntimeError(f"checkpoint {checksum_field} checksum does not match JSON")
    unsigned = line[: marker.start()] + "}"
    if sha256_bytes(unsigned.encode("utf-8")) != supplied:
        raise RuntimeError(f"checkpoint {checksum_field} checksum mismatch")


def _is_uint(value: Any, maximum: int) -> bool:
    return type(value) is int and 0 <= value <= maximum


def _require_uint(value: Any, maximum: int, field: str) -> None:
    if not _is_uint(value, maximum):
        raise RuntimeError(f"malformed compact checkpoint {field}")


def _require_object_fields(record: dict[str, Any], fields: tuple[str, ...], label: str) -> None:
    missing = [field for field in fields if field not in record]
    if missing:
        raise RuntimeError(f"malformed object checkpoint {label}: missing={missing}")


def _require_string_fields(record: dict[str, Any], fields: tuple[str, ...], label: str) -> None:
    _require_object_fields(record, fields, label)
    if any(not isinstance(record[field], str) for field in fields):
        raise RuntimeError(f"malformed object checkpoint {label}")


def _normalize_object_attribution(attribution: Any) -> None:
    if not isinstance(attribution, dict):
        raise RuntimeError("malformed object checkpoint attribution")
    _require_object_fields(
        attribution, ("start", "end", "author_id", "ts"), "attribution"
    )
    if not (
        _is_uint(attribution["start"], USIZE_MAX)
        and _is_uint(attribution["end"], USIZE_MAX)
        and isinstance(attribution["author_id"], str)
        and _is_uint(attribution["ts"], U128_MAX)
    ):
        raise RuntimeError("malformed object checkpoint attribution")


def _normalize_object_line_attribution(attribution: Any) -> None:
    if not isinstance(attribution, dict):
        raise RuntimeError("malformed object checkpoint line attribution")
    _require_object_fields(
        attribution,
        ("start_line", "end_line", "author_id"),
        "line attribution",
    )
    overrode = attribution.get("overrode")
    if not (
        _is_uint(attribution["start_line"], U32_MAX)
        and _is_uint(attribution["end_line"], U32_MAX)
        and isinstance(attribution["author_id"], str)
        and (overrode is None or isinstance(overrode, str))
    ):
        raise RuntimeError("malformed object checkpoint line attribution")


def _normalize_object_entry(entry: Any) -> dict[str, str]:
    if not isinstance(entry, dict):
        raise RuntimeError("malformed object checkpoint entry")
    _require_string_fields(entry, ("file",), "entry")
    blob_sha = entry.get("blob_sha", "")
    attributions = entry.get("attributions", [])
    line_attributions = entry.get("line_attributions", [])
    if not isinstance(blob_sha, str):
        raise RuntimeError("malformed object checkpoint entry blob")
    if not isinstance(attributions, list) or not isinstance(line_attributions, list):
        raise RuntimeError("malformed object checkpoint entry attributions")
    for attribution in attributions:
        _normalize_object_attribution(attribution)
    for attribution in line_attributions:
        _normalize_object_line_attribution(attribution)
    return {"file": entry["file"], "blob_sha": blob_sha}


def _normalize_object_line_stats(value: Any) -> dict[str, int]:
    if not isinstance(value, dict):
        raise RuntimeError("malformed object checkpoint line stats")
    normalized = {
        field: value.get(field, 0)
        for field in (
            "additions",
            "deletions",
            "additions_sloc",
            "deletions_sloc",
        )
    }
    if not all(_is_uint(count, U32_MAX) for count in normalized.values()):
        raise RuntimeError("malformed object checkpoint line stats")
    return normalized


def _validate_object_optional_structs(record: dict[str, Any]) -> None:
    agent_id = record.get("agent_id")
    if agent_id is not None:
        if not isinstance(agent_id, dict):
            raise RuntimeError("malformed object checkpoint agent id")
        _require_string_fields(agent_id, ("tool", "id", "model"), "agent id")

    metadata = record.get("agent_metadata")
    if metadata is not None and not (
        isinstance(metadata, dict)
        and all(isinstance(value, str) for value in metadata.values())
    ):
        raise RuntimeError("malformed object checkpoint agent metadata")

    known_human = record.get("known_human_metadata")
    if known_human is not None:
        if not isinstance(known_human, dict):
            raise RuntimeError("malformed object checkpoint known human metadata")
        _require_string_fields(
            known_human,
            ("editor", "editor_version", "extension_version"),
            "known human metadata",
        )

    for field in ("git_ai_version", "trace_id", "delivery_id"):
        value = record.get(field)
        if value is not None and not isinstance(value, str):
            raise RuntimeError(f"malformed object checkpoint field {field}")


def _normalize_object_checkpoint(record: dict[str, Any]) -> dict[str, Any]:
    _require_object_fields(
        record, ("diff", "author", "entries", "timestamp"), "record"
    )
    kind = record.get("kind", "Human")
    if kind not in CHECKPOINT_KINDS:
        raise RuntimeError(f"malformed object checkpoint kind {kind!r}")
    if not isinstance(record["diff"], str) or not isinstance(record["author"], str):
        raise RuntimeError("malformed object checkpoint author or diff")
    entries = record["entries"]
    if not isinstance(entries, list):
        raise RuntimeError("malformed object checkpoint entries")
    if not _is_uint(record["timestamp"], U64_MAX):
        raise RuntimeError("malformed object checkpoint timestamp")
    normalized_entries = [_normalize_object_entry(entry) for entry in entries]
    _validate_object_optional_structs(record)
    line_stats = _normalize_object_line_stats(record.get("line_stats", {}))
    api_version = record.get("api_version", "")
    if not isinstance(api_version, str):
        raise RuntimeError("malformed object checkpoint api version")
    if api_version != CHECKPOINT_API_VERSION:
        raise RuntimeError(f"unsupported checkpoint api version {api_version!r}")

    return {
        "kind": kind,
        "entries": normalized_entries,
        "agent_metadata": record.get("agent_metadata"),
        "line_stats": line_stats,
        "api_version": api_version,
        "trace_id": record.get("trace_id"),
        "delivery_id": record.get("delivery_id"),
    }


def _validate_optional_string(record: dict[str, Any], field: str) -> None:
    value = record.get(field)
    if value is not None and not isinstance(value, str):
        raise RuntimeError(f"malformed compact checkpoint field {field}")


def _validate_optional_string_triple(record: dict[str, Any], field: str) -> None:
    value = record.get(field)
    if value is not None and not (
        isinstance(value, list)
        and len(value) == 3
        and all(isinstance(item, str) for item in value)
    ):
        raise RuntimeError(f"malformed compact checkpoint field {field}")


def _validate_attribution(attribution: Any) -> None:
    if not (
        isinstance(attribution, list)
        and len(attribution) == 4
        and _is_uint(attribution[0], USIZE_MAX)
        and _is_uint(attribution[1], USIZE_MAX)
        and isinstance(attribution[2], str)
        and _is_uint(attribution[3], U128_MAX)
    ):
        raise RuntimeError("malformed compact checkpoint attribution")


def _validate_line_attribution(attribution: Any) -> None:
    if not (
        isinstance(attribution, list)
        and len(attribution) == 4
        and _is_uint(attribution[0], U32_MAX)
        and _is_uint(attribution[1], U32_MAX)
        and isinstance(attribution[2], str)
        and (attribution[3] is None or isinstance(attribution[3], str))
    ):
        raise RuntimeError("malformed compact checkpoint line attribution")


def _validate_entry(entry: Any) -> None:
    if not (
        isinstance(entry, list)
        and len(entry) == 4
        and isinstance(entry[0], str)
        and isinstance(entry[1], str)
        and isinstance(entry[2], list)
        and isinstance(entry[3], list)
    ):
        raise RuntimeError("malformed compact checkpoint entry")
    for attribution in entry[2]:
        _validate_attribution(attribution)
    for attribution in entry[3]:
        _validate_line_attribution(attribution)


def _normalize_v2_checkpoint(line: str, record: dict[str, Any]) -> dict[str, Any]:
    _verify_terminal_checksum(line, record, "c")
    version = record.get("v")
    if not _is_uint(version, U64_MAX) or version != 2:
        raise RuntimeError(f"unsupported compact checkpoint version {version!r}")

    unknown = set(record) - V2_ALLOWED_FIELDS
    missing = V2_REQUIRED_FIELDS - set(record)
    if unknown or missing:
        raise RuntimeError(
            "malformed compact checkpoint fields: "
            f"unknown={sorted(unknown)}, missing={sorted(missing)}"
        )
    kind = record["k"]
    if not _is_uint(kind, U8_MAX) or kind not in range(4):
        raise RuntimeError(f"invalid compact checkpoint kind {kind!r}")
    if not isinstance(record["a"], str) or not isinstance(record["d"], str):
        raise RuntimeError("malformed compact checkpoint author or diff")
    _require_uint(record["t"], U64_MAX, "timestamp")
    stats = record["s"]
    if not (
        isinstance(stats, list)
        and len(stats) == 4
        and all(_is_uint(value, U32_MAX) for value in stats)
    ):
        raise RuntimeError("malformed compact checkpoint line stats")
    for field in ("g", "r", "y"):
        _validate_optional_string(record, field)
    for field in ("h", "i"):
        _validate_optional_string_triple(record, field)
    metadata = record.get("m")
    if metadata is not None:
        if not isinstance(metadata, dict) or not all(
            isinstance(key, str) and isinstance(value, str)
            for key, value in metadata.items()
        ):
            raise RuntimeError("malformed compact checkpoint metadata")

    entries = record["e"]
    if not isinstance(entries, list):
        raise RuntimeError("malformed compact checkpoint entries")
    normalized_entries = []
    for entry in entries:
        _validate_entry(entry)
        normalized_entries.append({"file": entry[0], "blob_sha": entry[1]})

    return {
        "kind": ("Human", "AiAgent", "AiTab", "KnownHuman")[kind],
        "entries": normalized_entries,
        "trace_id": record.get("r"),
        "delivery_id": record.get("y"),
    }


def normalize_checkpoint_record(line: str) -> dict[str, Any]:
    record = _parse_checkpoint_record(line)
    if "v" in record:
        return _normalize_v2_checkpoint(line, record)
    if "c" in record:
        raise RuntimeError("checkpoint checksum has no compact record version")
    if V1_VERSION_FIELD in record:
        _verify_terminal_checksum(line, record, V1_CHECKSUM_FIELD)
        version = record[V1_VERSION_FIELD]
        if type(version) is not int or version != 1:
            raise RuntimeError(f"unsupported checkpoint record version {version!r}")
        return _normalize_object_checkpoint(record)
    if V1_CHECKSUM_FIELD in record:
        raise RuntimeError("checkpoint checksum has no record version")
    return _normalize_object_checkpoint(record)


def find_materialized_checkpoint(
    lines: list[str], *, expected_blob_sha: str, expected_path: str
) -> dict[str, Any]:
    matches: list[dict[str, Any]] = []
    for line in lines:
        if not line.strip():
            continue
        record = normalize_checkpoint_record(line)
        entries = record.get("entries")
        if record.get("kind") != "AiAgent" or not isinstance(entries, list):
            continue
        if len(entries) == 1 and (
            isinstance(entries[0], dict)
            and entries[0].get("file") == expected_path
            and entries[0].get("blob_sha") == expected_blob_sha
        ):
            matches.append(record)
    if len(matches) != 1:
        raise RuntimeError(
            "expected exactly one materialized AI checkpoint for "
            f"{expected_path} at blob {expected_blob_sha}, found {len(matches)}"
        )
    return matches[0]


def validate_materialized_blob(
    working_logs: Path, *, expected_blob_sha: str, expected_content: bytes
) -> Path:
    matches = list(working_logs.glob(f"*/blobs/{expected_blob_sha}"))
    if len(matches) != 1:
        raise RuntimeError(
            f"expected exactly one blob {expected_blob_sha}, found {len(matches)}"
        )
    if matches[0].read_bytes() != expected_content:
        raise RuntimeError(f"materialized blob content mismatch for {expected_blob_sha}")
    return matches[0]
