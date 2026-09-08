# Copyright 2026 The git-ai fork contributors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# Read-only format research derived from jj v0.45.1, Apache-2.0:
# https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/protos/simple_op_store.proto
# https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/simple_op_store.rs
# https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/op_store.rs
# https://github.com/jj-vcs/jj/blob/v0.45.1/core/src/content_hash.rs

"""Read-only proof for the jj 0.45.1 current operation-store format.

No backend or CLI is loaded. Unknown fields, unsupported legacy structures, and
content-address mismatches fail closed. The format profile does not establish the
writer version. Callers must bound total work and reject incomplete operation DAGs."""
import hashlib
import struct
from pathlib import Path
MAX = 16 * 1024 * 1024

def read_bounded(p):
    with p.open('rb') as f:
        data = f.read(MAX + 1)
    if len(data) > MAX:
        raise ValueError('oversize object')
    return data
ZERO = '0' * 128

def fields(b, allowed):
    if len(b) > MAX:
        raise ValueError('oversize protobuf')
    i = 0
    out = {}

    def varint():
        nonlocal i
        n = 0
        for shift in range(0, 70, 7):
            if i == len(b):
                raise ValueError('truncated varint')
            v = b[i]
            i += 1
            n |= (v & 127) << shift
            if not v & 128:
                if n > 2 ** 64 - 1:
                    raise ValueError('varint overflow')
                return n
        raise ValueError('varint overflow')
    while i < len(b):
        t = varint()
        tag = t >> 3
        wt = t & 7
        if tag not in allowed or allowed[tag] != wt:
            raise ValueError(f'unknown/wrong field {tag}/{wt}')
        if wt == 0:
            value = varint()
        elif wt == 2:
            size = varint()
            if size > len(b) - i:
                raise ValueError('truncated field')
            value = b[i:i + size]
            i += size
        else:
            raise ValueError('unsupported wire type')
        out.setdefault(tag, []).append(value)
    return out

def one(f, k, default=b''):
    values = f.get(k, [])
    if len(values) > 1:
        raise ValueError('duplicate singular')
    return values[0] if values else default

def seq(xs, enc):
    return struct.pack('<Q', len(xs)) + b''.join((enc(x) for x in xs))

def blob(x):
    return struct.pack('<Q', len(x)) + x

def option(x, enc):
    return struct.pack('<I', x is not None) + (enc(x) if x is not None else b'')

def mapping(d, enc):
    return struct.pack('<Q', len(d)) + b''.join((blob(k) + enc(v) for k, v in sorted(d.items())))

def oid(b, n):
    if len(b) != n:
        raise ValueError('wrong id length')
    return b

def timestamp(b):
    f = fields(b, {1: 0, 2: 0})
    return (one(f, 1, 0) % 2 ** 64).to_bytes(8, 'little') + (one(f, 2, 0) % 2 ** 32).to_bytes(4, 'little')

def metadata(b):
    f = fields(b, {1: 2, 2: 2, 3: 2, 4: 2, 5: 2, 6: 2, 7: 0, 8: 2})
    attrs = {}
    for a in f.get(6, []):
        m = fields(a, {1: 2, 2: 2})
        k = one(m, 1)
        v = one(m, 2)
        if k in attrs:
            raise ValueError('duplicate attribute')
        k.decode()
        v.decode()
        attrs[k] = v
    for k in (3, 4, 5, 8):
        one(f, k).decode()
    snap = one(f, 7, 0)
    if snap not in (0, 1):
        raise ValueError('invalid bool')
    encoded = (
        timestamp(one(f, 1))
        + timestamp(one(f, 2))
        + b''.join(blob(one(f, k)) for k in (3, 4, 5))
        + bytes([snap])
        + option(one(f, 8) if 8 in f else None, blob)
        + mapping(attrs, blob)
    )
    return (encoded, one(f, 8).decode(), bool(snap))

def decode_operation(path, check_hash=True, *, data=None):
    p = Path(path)
    b = read_bounded(p) if data is None else data
    f = fields(b, {1: 2, 2: 2, 3: 2, 4: 2, 5: 0})
    view = oid(one(f, 1), 64)
    parents = [oid(x, 64) for x in f.get(2, [])]
    if not parents:
        raise ValueError('unsupported legacy operation without parents')
    if len(set(parents)) != len(parents):
        raise ValueError('duplicate operation parent')
    m, ws, snap = metadata(one(f, 3))
    pred = {}
    for entry in f.get(4, []):
        e = fields(entry, {1: 2, 2: 2})
        k = oid(one(e, 1), 20)
        if k in pred:
            raise ValueError('duplicate predecessor commit')
        pred[k] = [oid(x, 20) for x in e.get(2, [])]
    stores = one(f, 5, 0)
    if stores not in (0, 1) or (not stores and pred):
        raise ValueError('invalid predecessor flag')
    encoded = (
        blob(view)
        + seq(parents, blob)
        + m
        + option(pred if stores else None, lambda d: mapping(d, lambda xs: seq(xs, blob)))
    )
    calculated = hashlib.blake2b(encoded).hexdigest()
    if check_hash and calculated != p.name:
        raise ValueError(f'operation hash mismatch {calculated}')
    return {
        'operation_id': p.name,
        'view_id': view.hex(),
        'parents': [x.hex() for x in parents],
        'workspace': ws,
        'is_snapshot': snap,
        'stores_commit_predecessors': bool(stores),
        'predecessors': {k.hex(): [x.hex() for x in v] for k, v in pred.items()},
        'domain_hash_verified': check_hash,
        'raw_bytes': len(b),
    }

def term(raw):
    f = fields(raw, {1: 2})
    return oid(one(f, 1), 20) if 1 in f else None

def target(raw):
    if raw is None:
        return [None]
    f = fields(raw, {1: 2, 2: 2, 3: 2})
    if set(f) != {3}:
        raise ValueError('unsupported legacy RefTarget')
    c = fields(one(f, 3), {1: 2, 2: 2})
    removes = [term(x) for x in c.get(1, [])]
    adds = [term(x) for x in c.get(2, [])]
    if len(adds) != len(removes) + 1:
        raise ValueError('invalid conflict arity')
    values = []
    for i, value in enumerate(adds):
        values.append(value)
        if i < len(removes):
            values.append(removes[i])
    return values

def merge_hash(values):
    return seq(values, lambda v: option(v, blob))

def put(d, k, v):
    if k in d:
        raise ValueError('duplicate map key')
    d[k] = v

def name(f, k=1):
    v = one(f, k)
    v.decode()
    return v

def remote_ref(raw):
    f = fields(raw, {1: 2, 2: 2, 3: 0})
    values = [term(x) for x in f.get(2, [])]
    state = one(f, 3, 0)
    if len(values) % 2 != 1:
        raise ValueError('invalid ref term count')
    if state not in (0, 1):
        raise ValueError('unknown remote state')
    return (name(f), (values, state))

def remote_hash(ref):
    return merge_hash(ref[0]) + struct.pack('<I', ref[1])

def decode_view(path, check_hash=True, *, data=None):
    p = Path(path)
    b = read_bounded(p) if data is None else data
    f = fields(b, {1: 2, 2: 2, 3: 2, 5: 2, 6: 2, 7: 2, 8: 2, 9: 2, 11: 2, 12: 0, 13: 2})
    wc = {}
    if one(f, 2) or one(f, 7) or one(f, 12, 0) != 1:
        raise ValueError('unsupported legacy view profile')
    for entry in f.get(8, []):
        e = fields(entry, {1: 2, 2: 2})
        put(wc, name(e), oid(one(e, 2), 20))
    heads = [oid(x, 20) for x in f.get(1, [])]
    if not heads:
        raise ValueError('view must contain a commit head')
    if len(set(heads)) != len(heads):
        raise ValueError('duplicate commit head')
    local = {}
    bookmark_names = set()
    legacy_remote = {}
    for raw in f.get(5, []):
        e = fields(raw, {1: 2, 2: 2, 3: 2})
        key = name(e)
        if key in bookmark_names:
            raise ValueError('duplicate bookmark name')
        bookmark_names.add(key)
        v = target(one(e, 2) if 2 in e else None)
        if v != [None]:
            put(local, key, v)
        for rr in e.get(3, []):
            r = fields(rr, {1: 2, 2: 2, 3: 0})
            rn = name(r)
            state = one(r, 3, 0)
            if state not in (0, 1):
                raise ValueError('unknown legacy remote state')
            rd = legacy_remote.setdefault(rn, ({}, {}))
            put(rd[0], key, (target(one(r, 2) if 2 in r else None), state))
    tags = {}
    for raw in f.get(6, []):
        e = fields(raw, {1: 2, 2: 2})
        put(tags, name(e), target(one(e, 2) if 2 in e else None))
    remote = {}
    for raw in f.get(11, []):
        e = fields(raw, {1: 2, 2: 2, 3: 2})
        bookmarks = {}
        rtags = {}
        for x in e.get(2, []):
            k, v = remote_ref(x)
            put(bookmarks, k, v)
        for x in e.get(3, []):
            k, v = remote_ref(x)
            put(rtags, k, v)
        put(remote, name(e), (bookmarks, rtags))
    if remote:
        current_bookmarks = {k: v[0] for k, v in remote.items() if v[0]}
        legacy_bookmarks = {k: v[0] for k, v in legacy_remote.items() if v[0]}
        if current_bookmarks != legacy_bookmarks:
            raise ValueError('inconsistent legacy remote mirror')
    elif legacy_remote:
        raise ValueError('unsupported legacy remote-only view')
    gitrefs = {}
    for raw in f.get(3, []):
        e = fields(raw, {1: 2, 2: 2, 3: 2})
        if one(e, 2) or 3 not in e:
            raise ValueError('unsupported legacy GitRef')
        put(gitrefs, name(e), target(one(e, 3)))
    githeads = {}
    for raw in f.get(13, []):
        e = fields(raw, {1: 2, 2: 2})
        put(githeads, name(e), target(one(e, 2) if 2 in e else None))
    if 9 in f:
        mirror = target(one(f, 9))
        if githeads.get(b'default') != mirror:
            raise ValueError('unsupported/inconsistent legacy Git HEAD')
    encoded = (
        seq(sorted(heads), blob)
        + mapping(local, merge_hash)
        + mapping(tags, merge_hash)
        + mapping(remote, lambda v: mapping(v[0], remote_hash) + mapping(v[1], remote_hash))
        + mapping(gitrefs, merge_hash)
        + mapping(githeads, merge_hash)
        + mapping(wc, blob)
    )
    calculated = hashlib.blake2b(encoded).hexdigest()
    if check_hash and calculated != p.name:
        raise ValueError(f'view hash mismatch {calculated}')
    ref_targets = [
        value
        for refs in (local, tags, gitrefs, githeads)
        for value in refs.values()
    ]
    ref_targets.extend(
        value[0]
        for pair in remote.values()
        for refs in pair
        for value in refs.values()
    )
    commit_references = len(heads) + len(wc)
    commit_references += sum(
        term is not None for values in ref_targets for term in values
    )
    return {
        'view_id': p.name,
        'heads': [x.hex() for x in sorted(heads)],
        'wc_commit_ids': {k.decode(): v.hex() for k, v in wc.items()},
        'domain_hash_verified': check_hash,
        'raw_bytes': len(b),
        'commit_references': commit_references,
        'counts': {
            'bookmarks': len(local),
            'tags': len(tags),
            'remotes': len(remote),
            'git_refs': len(gitrefs),
            'git_heads': len(githeads),
            'conflicted_refs': sum(len(values) > 1 for values in ref_targets),
        },
    }
