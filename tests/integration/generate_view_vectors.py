#!/usr/bin/env python3
"""Independent synthetic View oracle, derived from pinned jj v0.45.1.

Apache-2.0 source definitions: lib/src/op_store.rs:249-292,
lib/src/simple_op_store.rs:544-951, lib/src/protos/simple_op_store.proto,
core/src/content_hash.rs and core/src/merge.rs:186-197 at
https://github.com/jj-vcs/jj/tree/v0.45.1.
Only the existing synthetic operation oracle's wire/primitive helpers are reused.
No production decoder imports, repository content, or host metadata are used.
"""

import hashlib
import struct
from pathlib import Path

from generate_operation_vectors import blob, field, numbered_id, scalar


def sequence(values, encode):
    return struct.pack('<Q', len(values)) + b''.join(encode(v) for v in values)


def mapping(values, encode):
    return sequence(sorted(values.items()), lambda item: blob(item[0]) + encode(item[1]))


def term_wire(value):
    return b'' if value is None else field(1, value)


def target_wire(values):
    return field(3, b''.join(field(1, term_wire(v)) for v in values[1::2])
                 + b''.join(field(2, term_wire(v)) for v in values[::2]))


def target_hash(values):
    return sequence(values, lambda v: struct.pack('<I', int(v is not None))
                    + (b'' if v is None else blob(v)))


def remote_hash(ref):
    values, state = ref
    return target_hash(values) + struct.pack('<I', state)


def remote_wire(name, ref):
    values, state = ref
    return (field(1, name) + b''.join(field(2, term_wire(v)) for v in values)
            + scalar(3, state))


def view(*, heads=None, local=None, tags=None, remotes=None, gitrefs=None,
         githeads=None, wc=None, mirror_head=True):
    heads = [bytes([0xaa]) * 20] if heads is None else heads
    local, tags, remotes = local or {}, tags or {}, remotes or {}
    gitrefs, githeads, wc = gitrefs or {}, githeads or {}, wc or {}
    wire = b''.join(field(1, head) for head in heads)
    for name in sorted(set(local) | {k for refs, _ in remotes.values() for k in refs}):
        entry = field(1, name)
        if name in local:
            entry += field(2, target_wire(local[name]))
        for remote, (bookmarks, _) in remotes.items():
            if name in bookmarks:
                values, state = bookmarks[name]
                entry += field(3, field(1, remote) + field(2, target_wire(values))
                               + scalar(3, state))
        wire += field(5, entry)
    for name, values in tags.items():
        wire += field(6, field(1, name) + field(2, target_wire(values)))
    for name, (bookmarks, remote_tags) in remotes.items():
        entry = field(1, name)
        entry += b''.join(field(2, remote_wire(k, v)) for k, v in bookmarks.items())
        entry += b''.join(field(3, remote_wire(k, v)) for k, v in remote_tags.items())
        wire += field(11, entry)
    for name, values in gitrefs.items():
        wire += field(3, field(1, name) + field(3, target_wire(values)))
    for name, values in githeads.items():
        wire += field(13, field(1, name) + field(2, target_wire(values)))
    for name, commit in wc.items():
        wire += field(8, field(1, name) + field(2, commit))
    if mirror_head and b'default' in githeads:
        wire += field(9, target_wire(githeads[b'default']))
    wire += scalar(12, 1)
    retained_local = {k: v for k, v in local.items() if v != [None]}
    domain = sequence(sorted(heads), blob)
    domain += mapping(retained_local, target_hash) + mapping(tags, target_hash)
    domain += mapping(remotes, lambda r: mapping(r[0], remote_hash)
                      + mapping(r[1], remote_hash))
    domain += mapping(gitrefs, target_hash) + mapping(githeads, target_hash)
    domain += mapping(wc, blob)
    return wire, hashlib.blake2b(domain, digest_size=64).hexdigest()


def rich_model():
    a, b, c = (bytes([v]) * 20 for v in (0xaa, 0xbb, 0xcc))
    conflict = [a, None, b]
    return dict(
        heads=[b, a],
        local={b'conflict': conflict, 'local-工'.encode(): [c], b'deleted': [None]},
        tags={b'absent': [None], b'v1': [a]},
        remotes={
            b'origin': ({b'conflict': (conflict, 1), b'remoteonly': ([c], 0)},
                        {b'release': ([b], 1), b'tombstone': ([None], 0)}),
            b'tagonly': ({}, {b't': ([c], 1)}),
        },
        gitrefs={b'refs/heads/main': [a], b'refs/tags/gone': [None]},
        githeads={b'default': [a], 'workspace-工'.encode(): [None]},
        wc={b'default': b, 'workspace-工'.encode(): c},
    )


def all_vectors():
    a = bytes([0xaa]) * 20
    rich = rich_model()
    small = {
        'MINIMAL': view(),
        'RICH': view(**rich),
        'RICH_NO_HEAD_MIRROR': view(**rich, mirror_head=False),
        'ROOT_COMMIT': view(heads=[bytes(20)]),
        'ABSENT_LOCAL': view(local={b'deleted': [None]}),
        'ABSENT_TAG': view(tags={b'deleted': [None]}),
        'ABSENT_LOCAL_CONFLICT': view(local={b'deleted': [None, None, None]}),
        'ABSENT_GITREF': view(gitrefs={b'deleted': [None]}),
        'ABSENT_GITHEAD': view(githeads={b'default': [None]}),
        'ABSENT_REMOTE': view(remotes={b'r': ({}, {b't': ([None], 0)})}),
        'ZERO_COMMIT_TAG': view(tags={b'deleted': [bytes(20)]}),
        'REPEATED_TERMS': view(tags={b't': [a, a, a]}),
        'REMOTE_NEW': view(remotes={b'r': ({}, {b't': ([a], 0)})}),
        'REMOTE_TRACKED': view(remotes={b'r': ({}, {b't': ([a], 1)})}),
        'REMOTE_BOOKMARK_NEW': view(remotes={b'r': ({b'b': ([a], 0)}, {})}),
        'EMPTY_REMOTE': view(remotes={b'r': ({}, {})}),
        'EMPTY_WORKSPACE': view(wc={b'': a}),
    }
    limits = {
        'HEADS_LIMIT': view(heads=[numbered_id(i, 20) for i in range(4096)]),
        'REPEATED_REFERENCES_LIMIT': view(tags={b't': [a] * 4095}),
        'NAME_BYTES_LIMIT': view(tags={b'x' * (64 * 1024): [None]}),
        'UNICODE_NAME_BYTES_LIMIT': view(tags={b'x' * (64 * 1024 - 2) + 'é'.encode(): [None]}),
        'WIRE_ENTRIES_LIMIT': view(tags={b't': [None] * 8189}, local={b'empty': [None]}),
        'MIRROR_NAMES_LIMIT': view(remotes={b'r': ({b'x' * 32767: ([None], 0)}, {})}),
        'MIRROR_WIRE_LIMIT': view(
            remotes={b'r': ({b'b': ([a] * 4093, 1)}, {})}, local={b'empty': [None]}),
    }
    return small, limits


def vectors():
    small, limits = all_vectors()
    out = ['// Generated by generate_view_vectors.py; synthetic metadata only.', '']
    for name, (wire, digest) in small.items():
        out.append(f'pub const {name}_ID: &str = "{digest}";')
        hx = wire.hex()
        if len(hx) <= 96:
            out.append(f'pub const {name}_HEX: &str = "{hx}";\n')
        else:
            out.append(f'pub const {name}_HEX: &str = concat!(')
            out.extend(f'    "{hx[i:i+96]}",' for i in range(0, len(hx), 96))
            out.append(');\n')
    for name, (_, digest) in limits.items():
        out.append(f'pub const {name}_ID: &str = "{digest}";')
    return '\n'.join(out) + '\n'


if __name__ == '__main__':
    Path(__file__).with_name('jj_view_vectors.rs').write_text(vectors())
