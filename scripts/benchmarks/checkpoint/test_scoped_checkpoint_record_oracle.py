from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path


sys.path.insert(0, str(Path(__file__).parent))
import scoped_checkpoint_record as benchmark  # noqa: E402


def signed_checkpoint_record(record: dict[str, object], checksum_field: str) -> str:
    unsigned = json.dumps(record, separators=(",", ":"))
    checksum = benchmark.sha256_bytes(unsigned.encode("utf-8"))
    return unsigned[:-1] + f',"{checksum_field}":"{checksum}"' + "}"


def full_object_checkpoint(blob_sha: str) -> dict[str, object]:
    return {
        "kind": "AiAgent",
        "diff": "diff",
        "author": "author",
        "entries": [
            {
                "file": "sample.txt",
                "blob_sha": blob_sha,
                "attributions": [
                    {"start": 0, "end": 1, "author_id": "agent", "ts": 1}
                ],
                "line_attributions": [
                    {
                        "start_line": 1,
                        "end_line": 1,
                        "author_id": "agent",
                        "overrode": None,
                    }
                ],
            }
        ],
        "timestamp": 1,
        "agent_id": {"tool": "mock_ai", "id": "thread", "model": "model"},
        "agent_metadata": {"edit_kind": "file_edit"},
        "line_stats": {
            "additions": 1,
            "deletions": 0,
            "additions_sloc": 1,
            "deletions_sloc": 0,
        },
        "api_version": "checkpoint/1.0.0",
        "git_ai_version": "1.6.16",
        "known_human_metadata": {
            "editor": "vscode",
            "editor_version": "1.2.3",
            "extension_version": "4.5.6",
        },
        "trace_id": "trace-1",
        "delivery_id": "delivery-1",
    }


def object_checkpoint_lines(record: dict[str, object]) -> dict[str, str]:
    v1 = dict(record)
    v1[benchmark.V1_VERSION_FIELD] = 1
    return {
        "legacy": json.dumps(record, separators=(",", ":")),
        "v1": signed_checkpoint_record(v1, benchmark.V1_CHECKSUM_FIELD),
    }


def without_field(record: dict[str, object], field: str) -> dict[str, object]:
    result = dict(record)
    del result[field]
    return result


def valid_v2_checkpoint(blob_sha: str = "blob") -> dict[str, object]:
    return {
        "a": "author",
        "d": "diff",
        "e": [
            [
                "sample.txt",
                blob_sha,
                [[0, 1, "agent", 1]],
                [[1, 1, "agent", None]],
            ]
        ],
        "k": 1,
        "s": [1, 0, 1, 0],
        "t": 1,
        "v": 2,
    }


class CheckpointRecordOracleTests(unittest.TestCase):
    def test_materialized_blob_requires_exact_content(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            working_logs = Path(directory)
            blob_dir = working_logs / "base" / "blobs"
            blob_dir.mkdir(parents=True)
            content = b"expected\n"
            digest = benchmark.sha256_bytes(content)
            (blob_dir / digest).write_bytes(content)

            path = benchmark.validate_materialized_blob(
                working_logs, expected_blob_sha=digest, expected_content=content
            )

            self.assertEqual(path.name, digest)

    def test_materialized_blob_rejects_wrong_content(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            working_logs = Path(directory)
            blob_dir = working_logs / "base" / "blobs"
            blob_dir.mkdir(parents=True)
            content = b"expected\n"
            digest = benchmark.sha256_bytes(content)
            (blob_dir / digest).write_bytes(b"wrong\n")

            with self.assertRaisesRegex(RuntimeError, "content mismatch"):
                benchmark.validate_materialized_blob(
                    working_logs,
                    expected_blob_sha=digest,
                    expected_content=content,
                )

    def test_materialized_checkpoint_requires_one_expected_blob_and_path(self) -> None:
        expected = full_object_checkpoint("abc123")
        distractor = full_object_checkpoint("other")
        distractor["kind"] = "Human"
        lines = [
            object_checkpoint_lines(expected)["legacy"],
            object_checkpoint_lines(distractor)["legacy"],
        ]

        record = benchmark.find_materialized_checkpoint(
            lines, expected_blob_sha="abc123", expected_path="sample.txt"
        )

        self.assertEqual(record["trace_id"], "trace-1")

    def test_materialized_checkpoint_normalizes_v1_and_compact_v2(self) -> None:
        v1_blob = "a" * 64
        v2_blob = "b" * 64
        v1 = object_checkpoint_lines(full_object_checkpoint(v1_blob))["v1"]
        v2 = signed_checkpoint_record(
            {
                "a": "author",
                "d": "diff",
                "e": [["sample.txt", v2_blob, [], []]],
                "k": 1,
                "s": [0, 0, 0, 0],
                "t": 1,
                "v": 2,
                "r": "trace-2",
                "y": "delivery-2",
            },
            "c",
        )

        normalized_v1 = benchmark.find_materialized_checkpoint(
            [v1], expected_blob_sha=v1_blob, expected_path="sample.txt"
        )
        normalized_v2 = benchmark.find_materialized_checkpoint(
            [v2], expected_blob_sha=v2_blob, expected_path="sample.txt"
        )

        self.assertEqual(normalized_v1["trace_id"], "trace-1")
        self.assertEqual(normalized_v1["delivery_id"], "delivery-1")
        self.assertEqual(normalized_v2["trace_id"], "trace-2")
        self.assertEqual(normalized_v2["delivery_id"], "delivery-2")
        self.assertEqual(
            normalized_v2["entries"],
            [{"file": "sample.txt", "blob_sha": v2_blob}],
        )

    def test_materialized_checkpoint_accepts_full_legacy_and_v1_records(self) -> None:
        blob = "legacy-blob"
        for encoding, record in object_checkpoint_lines(
            full_object_checkpoint(blob)
        ).items():
            with self.subTest(encoding=encoding):
                normalized = benchmark.find_materialized_checkpoint(
                    [record], expected_blob_sha=blob, expected_path="sample.txt"
                )

                self.assertEqual(normalized["trace_id"], "trace-1")
                self.assertEqual(normalized["delivery_id"], "delivery-1")

    def test_materialized_checkpoint_rejects_reserved_checksum_without_version(
        self,
    ) -> None:
        blob = "legacy-blob"
        record = signed_checkpoint_record(
            full_object_checkpoint(blob),
            "c",
        )

        with self.assertRaisesRegex(RuntimeError, "checksum.*version"):
            benchmark.find_materialized_checkpoint(
                [record], expected_blob_sha=blob, expected_path="sample.txt"
            )

    def test_materialized_checkpoint_rejects_signed_v1_with_reserved_version(
        self,
    ) -> None:
        blob = "legacy-blob"
        record_data = full_object_checkpoint(blob)
        record_data[benchmark.V1_VERSION_FIELD] = 1
        record_data["v"] = 2
        record = signed_checkpoint_record(record_data, benchmark.V1_CHECKSUM_FIELD)

        with self.assertRaises(RuntimeError):
            benchmark.find_materialized_checkpoint(
                [record], expected_blob_sha=blob, expected_path="sample.txt"
            )

    def test_object_checkpoint_preserves_serde_defaults(self) -> None:
        blob = "defaulted-blob"
        record = full_object_checkpoint(blob)
        for field in (
            "kind",
            "agent_id",
            "agent_metadata",
            "line_stats",
            "git_ai_version",
            "known_human_metadata",
            "trace_id",
            "delivery_id",
        ):
            del record[field]
        entry = record["entries"][0]
        assert isinstance(entry, dict)
        for field in ("blob_sha", "attributions", "line_attributions"):
            del entry[field]

        for encoding, line in object_checkpoint_lines(record).items():
            with self.subTest(encoding=encoding):
                normalized = benchmark.normalize_checkpoint_record(line)

                self.assertEqual(normalized["kind"], "Human")
                self.assertEqual(normalized["agent_metadata"], None)
                self.assertEqual(
                    normalized["line_stats"],
                    {
                        "additions": 0,
                        "deletions": 0,
                        "additions_sloc": 0,
                        "deletions_sloc": 0,
                    },
                )
                self.assertEqual(normalized["api_version"], "checkpoint/1.0.0")
                self.assertEqual(
                    normalized["entries"],
                    [{"file": "sample.txt", "blob_sha": ""}],
                )

    def test_object_checkpoint_missing_api_defaults_to_unsupported_empty_string(
        self,
    ) -> None:
        record = without_field(full_object_checkpoint("blob"), "api_version")

        for encoding, line in object_checkpoint_lines(record).items():
            with self.subTest(encoding=encoding):
                with self.assertRaisesRegex(
                    RuntimeError, "unsupported checkpoint api version ''"
                ):
                    benchmark.normalize_checkpoint_record(line)

    def test_object_checkpoint_rejects_missing_required_fields(self) -> None:
        valid = full_object_checkpoint("blob")
        entry = valid["entries"][0]
        assert isinstance(entry, dict)
        attribution = entry["attributions"][0]
        line_attribution = entry["line_attributions"][0]
        agent_id = valid["agent_id"]
        known_human = valid["known_human_metadata"]
        assert isinstance(attribution, dict)
        assert isinstance(line_attribution, dict)
        assert isinstance(agent_id, dict)
        assert isinstance(known_human, dict)
        cases = {
            "diff": without_field(valid, "diff"),
            "author": without_field(valid, "author"),
            "entries": without_field(valid, "entries"),
            "timestamp": without_field(valid, "timestamp"),
            "entry file": {
                **valid,
                "entries": [without_field(entry, "file")],
            },
            "attribution start": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [without_field(attribution, "start")],
                    }
                ],
            },
            "attribution end": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [without_field(attribution, "end")],
                    }
                ],
            },
            "attribution author": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [without_field(attribution, "author_id")],
                    }
                ],
            },
            "attribution timestamp": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [without_field(attribution, "ts")],
                    }
                ],
            },
            "line attribution start": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            without_field(line_attribution, "start_line")
                        ],
                    }
                ],
            },
            "line attribution end": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            without_field(line_attribution, "end_line")
                        ],
                    }
                ],
            },
            "line attribution author": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            without_field(line_attribution, "author_id")
                        ],
                    }
                ],
            },
            "agent tool": {**valid, "agent_id": without_field(agent_id, "tool")},
            "agent id": {**valid, "agent_id": without_field(agent_id, "id")},
            "agent model": {
                **valid,
                "agent_id": without_field(agent_id, "model"),
            },
            "known human editor": {
                **valid,
                "known_human_metadata": without_field(known_human, "editor"),
            },
            "known human editor version": {
                **valid,
                "known_human_metadata": without_field(
                    known_human, "editor_version"
                ),
            },
            "known human extension version": {
                **valid,
                "known_human_metadata": without_field(
                    known_human, "extension_version"
                ),
            },
        }

        for label, record in cases.items():
            for encoding, line in object_checkpoint_lines(record).items():
                with self.subTest(label=label, encoding=encoding):
                    with self.assertRaises(RuntimeError):
                        benchmark.normalize_checkpoint_record(line)

    def test_object_checkpoint_rejects_field_types_and_unsigned_bounds(self) -> None:
        valid = full_object_checkpoint("blob")
        entry = valid["entries"][0]
        assert isinstance(entry, dict)
        attribution = entry["attributions"][0]
        line_attribution = entry["line_attributions"][0]
        stats = valid["line_stats"]
        agent_id = valid["agent_id"]
        known_human = valid["known_human_metadata"]
        assert isinstance(attribution, dict)
        assert isinstance(line_attribution, dict)
        assert isinstance(stats, dict)
        assert isinstance(agent_id, dict)
        assert isinstance(known_human, dict)
        cases = {
            "kind": {**valid, "kind": "ai_agent"},
            "diff": {**valid, "diff": 7},
            "author": {**valid, "author": []},
            "entries": {**valid, "entries": {}},
            "timestamp boolean": {**valid, "timestamp": True},
            "timestamp negative": {**valid, "timestamp": -1},
            "timestamp overflow": {**valid, "timestamp": 2**64},
            "entry": {**valid, "entries": ["entry"]},
            "entry file": {**valid, "entries": [{**entry, "file": 7}]},
            "entry blob": {**valid, "entries": [{**entry, "blob_sha": 7}]},
            "entry attributions": {
                **valid,
                "entries": [{**entry, "attributions": {}}],
            },
            "entry line attributions": {
                **valid,
                "entries": [{**entry, "line_attributions": {}}],
            },
            "attribution start boolean": {
                **valid,
                "entries": [
                    {**entry, "attributions": [{**attribution, "start": True}]}
                ],
            },
            "attribution start negative": {
                **valid,
                "entries": [
                    {**entry, "attributions": [{**attribution, "start": -1}]}
                ],
            },
            "attribution start overflow": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [
                            {**attribution, "start": benchmark.USIZE_MAX + 1}
                        ],
                    }
                ],
            },
            "attribution end overflow": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [
                            {**attribution, "end": benchmark.USIZE_MAX + 1}
                        ],
                    }
                ],
            },
            "attribution author": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [{**attribution, "author_id": 7}],
                    }
                ],
            },
            "attribution timestamp overflow": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "attributions": [{**attribution, "ts": 2**128}],
                    }
                ],
            },
            "line start overflow": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            {**line_attribution, "start_line": 2**32}
                        ],
                    }
                ],
            },
            "line end negative": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            {**line_attribution, "end_line": -1}
                        ],
                    }
                ],
            },
            "line author": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            {**line_attribution, "author_id": 7}
                        ],
                    }
                ],
            },
            "line overrode": {
                **valid,
                "entries": [
                    {
                        **entry,
                        "line_attributions": [
                            {**line_attribution, "overrode": 7}
                        ],
                    }
                ],
            },
            "agent": {**valid, "agent_id": []},
            "agent tool": {**valid, "agent_id": {**agent_id, "tool": 7}},
            "agent metadata": {**valid, "agent_metadata": []},
            "agent metadata value": {
                **valid,
                "agent_metadata": {"edit_kind": None},
            },
            "line stats": {**valid, "line_stats": []},
            "line stats boolean": {
                **valid,
                "line_stats": {**stats, "additions": True},
            },
            "line stats overflow": {
                **valid,
                "line_stats": {**stats, "deletions_sloc": 2**32},
            },
            "api version": {**valid, "api_version": 7},
            "git version": {**valid, "git_ai_version": 7},
            "known human": {**valid, "known_human_metadata": []},
            "known human editor": {
                **valid,
                "known_human_metadata": {**known_human, "editor": 7},
            },
            "trace": {**valid, "trace_id": 7},
            "delivery": {**valid, "delivery_id": []},
        }

        for label, record in cases.items():
            for encoding, line in object_checkpoint_lines(record).items():
                with self.subTest(label=label, encoding=encoding):
                    with self.assertRaises(RuntimeError):
                        benchmark.normalize_checkpoint_record(line)

    def test_object_checkpoint_accepts_rust_unsigned_boundaries(self) -> None:
        record = full_object_checkpoint("blob")
        entry = record["entries"][0]
        stats = record["line_stats"]
        assert isinstance(entry, dict)
        assert isinstance(stats, dict)
        entry["attributions"] = [
            {
                "start": benchmark.USIZE_MAX,
                "end": benchmark.USIZE_MAX,
                "author_id": "agent",
                "ts": 2**128 - 1,
            }
        ]
        entry["line_attributions"] = [
            {
                "start_line": 2**32 - 1,
                "end_line": 2**32 - 1,
                "author_id": "agent",
            }
        ]
        record["timestamp"] = 2**64 - 1
        record["line_stats"] = {key: 2**32 - 1 for key in stats}

        for encoding, line in object_checkpoint_lines(record).items():
            with self.subTest(encoding=encoding):
                normalized = benchmark.normalize_checkpoint_record(line)

                self.assertEqual(normalized["entries"][0]["blob_sha"], "blob")

    def test_object_checkpoint_rejects_unsupported_api_version(self) -> None:
        record = full_object_checkpoint("blob")
        record["api_version"] = "checkpoint/99.0.0"

        for encoding, line in object_checkpoint_lines(record).items():
            with self.subTest(encoding=encoding):
                with self.assertRaisesRegex(
                    RuntimeError, "unsupported checkpoint api version"
                ):
                    benchmark.normalize_checkpoint_record(line)

    def test_materialized_checkpoint_accepts_explicit_null_options(self) -> None:
        blob = "a" * 64
        record_data = valid_v2_checkpoint(blob)
        record_data.update(
            {"g": None, "h": None, "i": None, "m": None, "r": None, "y": None}
        )
        record = signed_checkpoint_record(record_data, "c")

        normalized = benchmark.find_materialized_checkpoint(
            [record], expected_blob_sha=blob, expected_path="sample.txt"
        )

        self.assertIsNone(normalized["trace_id"])
        self.assertIsNone(normalized["delivery_id"])

    def test_materialized_checkpoint_accepts_rust_unsigned_boundaries(self) -> None:
        blob = "a" * 64
        record_data = valid_v2_checkpoint(blob)
        record_data["e"] = [
            [
                "sample.txt",
                blob,
                [[sys.maxsize * 2 + 1, sys.maxsize * 2 + 1, "agent", 2**128 - 1]],
                [[2**32 - 1, 2**32 - 1, "agent", "previous"]],
            ]
        ]
        record_data["s"] = [2**32 - 1] * 4
        record_data["t"] = 2**64 - 1
        record = signed_checkpoint_record(record_data, "c")

        normalized = benchmark.find_materialized_checkpoint(
            [record], expected_blob_sha=blob, expected_path="sample.txt"
        )

        self.assertEqual(normalized["entries"][0]["blob_sha"], blob)

    def test_materialized_checkpoint_rejects_nested_tuple_and_integer_errors(
        self,
    ) -> None:
        blob = "a" * 64
        valid = valid_v2_checkpoint(blob)
        cases = {
            "short attribution": [["sample.txt", blob, [[0, 1, "agent"]], []]],
            "long attribution": [
                ["sample.txt", blob, [[0, 1, "agent", 1, "extra"]], []]
            ],
            "negative usize": [["sample.txt", blob, [[-1, 1, "agent", 1]], []]],
            "usize overflow": [
                ["sample.txt", blob, [[sys.maxsize * 2 + 2, 1, "agent", 1]], []]
            ],
            "boolean usize": [["sample.txt", blob, [[True, 1, "agent", 1]], []]],
            "u128 overflow": [
                ["sample.txt", blob, [[0, 1, "agent", 2**128]], []]
            ],
            "negative u128": [["sample.txt", blob, [[0, 1, "agent", -1]], []]],
            "boolean u128": [["sample.txt", blob, [[0, 1, "agent", True]], []]],
            "short line attribution": [
                ["sample.txt", blob, [], [[1, 1, "agent"]]]
            ],
            "long line attribution": [
                ["sample.txt", blob, [], [[1, 1, "agent", None, "extra"]]]
            ],
            "negative u32": [["sample.txt", blob, [], [[-1, 1, "agent", None]]]],
            "u32 overflow": [
                ["sample.txt", blob, [], [[2**32, 1, "agent", None]]]
            ],
            "invalid overrode": [["sample.txt", blob, [], [[1, 1, "agent", 7]]]],
        }
        record_cases = {
            label: signed_checkpoint_record({**valid, "e": entries}, "c")
            for label, entries in cases.items()
        }
        record_cases.update(
            {
                "u32 stats overflow": signed_checkpoint_record(
                    {**valid, "s": [2**32, 0, 0, 0]}, "c"
                ),
                "negative stats": signed_checkpoint_record(
                    {**valid, "s": [-1, 0, 0, 0]}, "c"
                ),
                "u64 timestamp overflow": signed_checkpoint_record(
                    {**valid, "t": 2**64}, "c"
                ),
                "negative timestamp": signed_checkpoint_record(
                    {**valid, "t": -1}, "c"
                ),
            }
        )

        for label, record in record_cases.items():
            with self.subTest(label=label):
                with self.assertRaises(RuntimeError):
                    benchmark.find_materialized_checkpoint(
                        [record], expected_blob_sha=blob, expected_path="sample.txt"
                    )

    def test_materialized_checkpoint_rejects_compact_field_type_errors(self) -> None:
        blob = "a" * 64
        valid = valid_v2_checkpoint(blob)
        cases = {
            "author": {**valid, "a": 7},
            "diff": {**valid, "d": []},
            "entries": {**valid, "e": {}},
            "entry path": {**valid, "e": [[7, blob, [], []]]},
            "entry blob": {**valid, "e": [["sample.txt", 7, [], []]]},
            "attributions": {**valid, "e": [["sample.txt", blob, {}, []]]},
            "line attributions": {**valid, "e": [["sample.txt", blob, [], {}]]},
            "attribution author": {
                **valid,
                "e": [["sample.txt", blob, [[0, 1, 7, 1]], []]],
            },
            "line author": {
                **valid,
                "e": [["sample.txt", blob, [], [[1, 1, 7, None]]]],
            },
            "git version": {**valid, "g": 7},
            "known human width": {**valid, "h": ["editor", "version"]},
            "known human value": {**valid, "h": ["editor", 7, "extension"]},
            "agent width": {**valid, "i": ["tool", "id"]},
            "agent value": {**valid, "i": ["tool", "id", 7]},
            "metadata shape": {**valid, "m": []},
            "metadata value": {**valid, "m": {"key": None}},
            "trace": {**valid, "r": 7},
            "delivery": {**valid, "y": []},
            "stats width": {**valid, "s": [0, 0, 0]},
            "stats boolean": {**valid, "s": [True, 0, 0, 0]},
            "timestamp boolean": {**valid, "t": True},
        }

        for label, record_data in cases.items():
            with self.subTest(label=label):
                record = signed_checkpoint_record(record_data, "c")
                with self.assertRaises(RuntimeError):
                    benchmark.normalize_checkpoint_record(record)

    def test_materialized_checkpoint_uses_rust_kind_codes(self) -> None:
        for kind, expected in enumerate(
            ("Human", "AiAgent", "AiTab", "KnownHuman")
        ):
            with self.subTest(kind=kind):
                record = signed_checkpoint_record(
                    {**valid_v2_checkpoint(), "k": kind}, "c"
                )

                self.assertEqual(
                    benchmark.normalize_checkpoint_record(record)["kind"], expected
                )

    def test_materialized_checkpoint_requires_lowercase_terminal_checksum(self) -> None:
        blob = "a" * 64
        record = signed_checkpoint_record(valid_v2_checkpoint(blob), "c")
        checksum_start = record.rfind('"c":"') + len('"c":"')
        malformed = (
            record[:checksum_start]
            + record[checksum_start : checksum_start + 64].upper()
            + record[checksum_start + 64 :]
        )

        with self.assertRaisesRegex(RuntimeError, "checksum"):
            benchmark.find_materialized_checkpoint(
                [malformed], expected_blob_sha=blob, expected_path="sample.txt"
            )

    def test_materialized_checkpoint_accepts_arbitrary_blob_string(self) -> None:
        blob = "Not-A-Lowercase-SHA"
        record = signed_checkpoint_record(valid_v2_checkpoint(blob), "c")

        normalized = benchmark.find_materialized_checkpoint(
            [record], expected_blob_sha=blob, expected_path="sample.txt"
        )

        self.assertEqual(normalized["entries"][0]["blob_sha"], blob)

    def test_materialized_checkpoint_rejects_tampered_compact_v2(self) -> None:
        original_blob = "a" * 64
        tampered_blob = "b" * 64
        record = signed_checkpoint_record(
            {
                "a": "author",
                "d": "diff",
                "e": [["sample.txt", original_blob, [], []]],
                "k": 1,
                "s": [0, 0, 0, 0],
                "t": 1,
                "v": 2,
            },
            "c",
        ).replace(original_blob, tampered_blob)

        with self.assertRaisesRegex(RuntimeError, "checksum"):
            benchmark.find_materialized_checkpoint(
                [record], expected_blob_sha=tampered_blob, expected_path="sample.txt"
            )

    def test_materialized_checkpoint_rejects_unknown_compact_version(self) -> None:
        blob = "a" * 64
        record = signed_checkpoint_record(
            {
                "a": "author",
                "d": "diff",
                "e": [["sample.txt", blob, [], []]],
                "k": 1,
                "s": [0, 0, 0, 0],
                "t": 1,
                "v": 99,
            },
            "c",
        )

        with self.assertRaisesRegex(RuntimeError, "version"):
            benchmark.find_materialized_checkpoint(
                [record], expected_blob_sha=blob, expected_path="sample.txt"
            )

    def test_materialized_checkpoint_rejects_malformed_compact_v2(self) -> None:
        blob = "a" * 64
        valid = {
            "a": "author",
            "d": "diff",
            "e": [["sample.txt", blob, [], []]],
            "k": 1,
            "s": [0, 0, 0, 0],
            "t": 1,
            "v": 2,
        }
        cases = {
            "boolean version": signed_checkpoint_record({**valid, "v": True}, "c"),
            "version overflow": signed_checkpoint_record(
                {**valid, "v": 2**64}, "c"
            ),
            "boolean kind": signed_checkpoint_record({**valid, "k": True}, "c"),
            "kind u8 overflow": signed_checkpoint_record(
                {**valid, "k": 2**8}, "c"
            ),
            "unknown key": signed_checkpoint_record({**valid, "z": 1}, "c"),
            "short entry": signed_checkpoint_record(
                {**valid, "e": [["sample.txt", blob, []]]}, "c"
            ),
            "non-finite": signed_checkpoint_record({**valid, "t": float("nan")}, "c"),
        }
        unsigned = json.dumps(valid, separators=(",", ":"))
        duplicate_unsigned = unsigned[:-1] + ',"v":2}'
        duplicate_checksum = benchmark.sha256_bytes(duplicate_unsigned.encode("utf-8"))
        cases["duplicate key"] = (
            duplicate_unsigned[:-1]
            + ',"c":"'
            + duplicate_checksum
            + '"}'
        )
        missing = dict(valid)
        del missing["d"]
        cases["missing key"] = signed_checkpoint_record(missing, "c")
        cases["non-terminal checksum"] = signed_checkpoint_record(valid, "c") + " "

        for label, record in cases.items():
            with self.subTest(label=label):
                with self.assertRaises(RuntimeError):
                    benchmark.normalize_checkpoint_record(record)

    def test_materialized_checkpoint_rejects_duplicates(self) -> None:
        record = object_checkpoint_lines(full_object_checkpoint("abc123"))["legacy"]

        with self.assertRaisesRegex(RuntimeError, "exactly one"):
            benchmark.find_materialized_checkpoint(
                [record, record], expected_blob_sha="abc123", expected_path="sample.txt"
            )

    def test_materialized_checkpoint_rejects_extra_entries(self) -> None:
        checkpoint = full_object_checkpoint("abc123")
        distractor = full_object_checkpoint("other")["entries"][0]
        assert isinstance(distractor, dict)
        distractor["file"] = "distractor.txt"
        entries = checkpoint["entries"]
        assert isinstance(entries, list)
        entries.append(distractor)
        record = object_checkpoint_lines(checkpoint)["legacy"]

        with self.assertRaisesRegex(RuntimeError, "exactly one"):
            benchmark.find_materialized_checkpoint(
                [record], expected_blob_sha="abc123", expected_path="sample.txt"
            )


if __name__ == "__main__":
    unittest.main()
