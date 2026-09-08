#!/usr/bin/env python3
"""Large metadata fixture from the existing independent jj v0.45.1 oracles."""
import argparse
import importlib
from pathlib import Path
import sys

sys.dont_write_bytecode = True


def generate(oracle_dir):
    sys.path.insert(0, str(oracle_dir))
    op = importlib.import_module("generate_operation_vectors")
    view = importlib.import_module("generate_view_vectors")
    workspace = b"\x01" * 16000
    view_wire, view_id = view.view(heads=[b"\xbb" * 20], wc={workspace: b"\xbb" * 20})
    anchors = []
    for index in range(32):
        wire, identity = op.operation(
            meta=op.metadata(description=f"output anchor {index:02}".encode()),
            parents=[bytes(64)], predecessors=[], view=bytes.fromhex(view_id))
        anchors.append((wire, identity))
    parents = [bytes.fromhex(identity) for _, identity in anchors]
    heads = []
    for index in range(32):
        wire, identity = op.operation(
            meta=op.metadata(description=f"output head {index:02}".encode()),
            parents=parents, predecessors=[], view=bytes.fromhex(view_id))
        heads.append((wire, identity))
    assert len({identity for _, identity in anchors + heads}) == 64
    assert max(len(wire) + len(view_wire) for wire, _ in anchors + heads) < 32768
    assert sum(len(wire) + len(view_wire) for wire, _ in heads) < 8 * 1024 * 1024
    lines = ["// Generated independently by generate_head_closure_output_vectors.py.",
             f'pub const VIEW_ID: &str = "{view_id}";', ""]
    for name, records in [("ANCHORS", anchors), ("HEADS", heads)]:
        lines.append(f"pub const {name}: [&str; 32] = [")
        lines.extend(f'    "{identity}",' for _, identity in records)
        lines.extend(["];", ""])
    return "\n".join(lines)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.write_text(generate(args.oracle_dir))
