"""Bounded, read-only experiment for the explicit jj 0.45.1 store profile.

This is research code, not a native-attribution capability or daemon adapter.
"""

from dataclasses import dataclass
import os
from pathlib import Path
import stat
import time

from decode_store import decode_operation, decode_view, fields, one, oid

PROFILE = "jj-simple-op-store/0.45.1"
ROOT = "0" * 128


class ReadError(ValueError):
    pass


@dataclass(frozen=True)
class Limits:
    operations: int = 256
    views: int = 256
    heads: int = 32
    predecessors: int = 4096
    commit_references: int = 4096
    record_bytes: int = 2 * 1024 * 1024
    total_bytes: int = 16 * 1024 * 1024
    elapsed_ms: int = 250


def operation_id(value):
    if len(value) != 128 or any(c not in "0123456789abcdef" for c in value):
        raise ReadError("invalid_operation_id")
    return value


class Reader:
    def __init__(self, repo, limits):
        self.repo = Path(repo)
        self.limits = limits
        self.started = time.monotonic()
        self.bytes = 0
        self.reads = 0
        self.operations = {}
        self.views = {}
        self.predecessors = 0
        self.commit_references = 0

    def checkpoint(self):
        # Cooperative elapsed budget; a blocked filesystem call is not preempted.
        if (time.monotonic() - self.started) * 1000 > self.limits.elapsed_ms:
            raise ReadError("elapsed_budget")

    def read(self, path):
        self.checkpoint()
        available = min(self.limits.record_bytes, self.limits.total_bytes - self.bytes)
        if available < 0:
            raise ReadError("byte_budget")
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
        with os.fdopen(os.open(path, flags), "rb") as stream:
            if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                raise ReadError("nonregular_metadata")
            data = stream.read(available + 1)
        self.reads += 1
        if len(data) > available:
            raise ReadError("byte_budget")
        self.bytes += len(data)
        self.checkpoint()
        return data

    def heads(self):
        heads = []
        with os.scandir(self.repo / "op_heads/heads") as entries:
            for count, entry in enumerate(entries, 1):
                self.checkpoint()
                if count > self.limits.heads + 1:
                    raise ReadError("head_budget")
                if entry.name == "lock":
                    continue
                operation_id(entry.name)
                if not entry.is_file(follow_symlinks=False) or entry.stat().st_size:
                    raise ReadError("invalid_head_marker")
                heads.append(entry.name)
                if len(heads) > self.limits.heads:
                    raise ReadError("head_budget")
        if not heads:
            raise ReadError("empty_heads")
        return sorted(heads)

    def operation(self, identity):
        operation_id(identity)
        if identity not in self.operations:
            if len(self.operations) >= self.limits.operations:
                raise ReadError("operation_budget")
            path = self.repo / "op_store/operations" / identity
            operation = decode_operation(path, data=self.read(path))
            if not operation["stores_commit_predecessors"]:
                raise ReadError("unrecorded_predecessors")
            if not operation["parents"] or len(operation["parents"]) > self.limits.heads:
                raise ReadError("parent_budget")
            self.predecessors += sum(1 + len(ids) for ids in operation["predecessors"].values())
            if self.predecessors > self.limits.predecessors:
                raise ReadError("predecessor_budget")
            self.operations[identity] = operation
            self.view(operation["view_id"])
        return self.operations[identity]

    def view(self, identity):
        operation_id(identity)
        if identity not in self.views:
            if len(self.views) >= self.limits.views:
                raise ReadError("view_budget")
            path = self.repo / "op_store/views" / identity
            view = decode_view(path, data=self.read(path))
            self.commit_references += view["commit_references"]
            if self.commit_references > self.limits.commit_references:
                raise ReadError("commit_reference_budget")
            self.views[identity] = view
        return self.views[identity]


def read_batch(repo, *, profile, is_known_operation=None, working_copy=None, limits=Limits()):
    if profile != PROFILE:
        raise ReadError("unsupported_profile")
    if any(value <= 0 for value in vars(limits).values()):
        raise ReadError("invalid_limits")
    known = is_known_operation or (lambda identity: identity == ROOT)
    known_cache = {ROOT: True}
    reader = Reader(repo, limits)
    for name, expected in (
        ("store/type", b"git"),
        ("op_store/type", b"simple_op_store"),
        ("op_heads/type", b"simple_op_heads_store"),
    ):
        if reader.read(reader.repo / name) != expected:
            raise ReadError("unsupported_backend")
    heads = reader.heads()
    checkout_path = Path(working_copy) / "checkout" if working_copy else None
    if working_copy and reader.read(Path(working_copy) / "type") != b"local":
        raise ReadError("unsupported_backend")
    checkout_bytes = reader.read(checkout_path) if checkout_path else None
    pending = [(identity, False) for identity in reversed(heads)]
    visiting, complete, reached = set(), set(), set()
    ordered = []
    while pending:
        reader.checkpoint()
        identity, expanded = pending.pop()
        if identity in complete:
            continue
        if identity not in known_cache:
            known_cache[identity] = bool(known(identity))
        if known_cache[identity]:
            reached.add(identity)
            continue
        if expanded:
            visiting.remove(identity)
            complete.add(identity)
            ordered.append(identity)
            continue
        if identity in visiting:
            raise ReadError("operation_cycle")
        visiting.add(identity)
        operation = reader.operation(identity)
        pending.append((identity, True))
        pending.extend((parent, False) for parent in reversed(operation["parents"]))
    checkout = None
    if checkout_bytes is not None:
        message = fields(checkout_bytes, {2: 2, 3: 2})
        identity = oid(one(message, 2), 64).hex()
        workspace = one(message, 3).decode("utf-8")
        if not workspace or identity == ROOT:
            raise ReadError("invalid_checkout")
        operation = reader.operation(identity)
        view = reader.view(operation["view_id"])
        commit = view["wc_commit_ids"].get(workspace)
        if commit is None:
            raise ReadError("missing_checkout_workspace")
        checkout = dict(operation_id=identity, workspace=workspace, commit_id=commit,
                        in_captured_history=identity in complete or identity in reached)
    if reader.heads() != heads:
        raise ReadError("heads_changed")
    if checkout_path and reader.read(checkout_path) != checkout_bytes:
        raise ReadError("checkout_changed")
    reader.checkpoint()
    return dict(profile=profile, capability="research_only", sampled_heads=heads,
                known_boundaries=sorted(reached),
                operations=[reader.operations[i] for i in ordered],
                views=list(reader.views.values()), checkout=checkout,
                counters=dict(operations=len(reader.operations), views=len(reader.views),
                              metadata_reads=reader.reads, bytes=reader.bytes,
                              predecessor_entries=reader.predecessors, subprocesses=0,
                              commit_references=reader.commit_references,
                              git_object_reads=0))
