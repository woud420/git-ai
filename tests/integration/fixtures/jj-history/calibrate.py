#!/usr/bin/env python3
"""Check generated expected IDs with the separate retained reader and wire encoder.

This is fixture calibration, not Rust runtime qualification. Large wire records
are reconstructed in memory, never written as committed blobs.
"""
import argparse
import hashlib
import importlib.util
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def varint(value):
    result = bytearray()
    while value > 127:
        result.append(128 + value % 128)
        value //= 128
    result.append(value)
    return bytes(result)


def field(tag, value):
    return varint(tag * 8 + 2) + varint(len(value)) + value


def reconstructed(parent, description, target, pred_edges=None):
    timestamp = bytes([8, 0, 16, 0])
    meta = field(1, timestamp) + field(2, timestamp) + field(3, description)
    meta += field(4, b'') + field(5, b'') + bytes([56, 0])
    raw = field(1, bytes.fromhex(target[1])) + field(2, bytes.fromhex(parent)) + field(3, meta)
    if pred_edges is not None:
        entry = field(1, bytes([0xaa]) * 20) + field(2, bytes([0xbb]) * 20) * pred_edges
        raw += field(4, entry)
    return raw + bytes([40, 1])


def main(oracle_dir, reader_path, output):
    generator = module('history_generator', Path(__file__).with_name('generate.py'))
    data = generator.fixtures(oracle_dir)
    reader = module('history_reader', reader_path)
    assert generator.output(data) == Path(__file__).with_name('vectors.rs').read_text()
    records = list(data['base'].values()) + list(data['small'].values())
    records += data['chain'] + data['raw'] + [data['raw_over'], data['base_tail'], data['base_tail_over']]
    records += data['predecessors'] + data['views']
    unique = {item[1]: item for item in records}
    assert len(unique) == len(records)
    views = {}
    decoded = {}
    for wire, identity, parents, target in records:
        assert len(identity) == 128 and identity == identity.lower()
        assert 0 < len(parents) <= 32
        assert len(wire) + len(target[0]) <= 1024 * 1024
        operation = reader.decode_operation(identity, data=wire)
        view = reader.decode_view(target[1], data=target[0])
        assert operation['parents'] == parents and operation['view_id'] == target[1]
        assert operation['domain_hash_verified'] and view['domain_hash_verified']
        assert sum(1 + len(edges) for edges in operation['predecessors'].values()) <= 4096
        assert view['commit_references'] <= 4096
        decoded[identity] = (operation, view)
        if target[1] in views:
            assert views[target[1]] == target[0]
        views[target[1]] = target[0]
    for item in data['chain']:
        assert item[0] == reconstructed(item[2][0], b'chain', item[3])
    for item in data['raw']:
        assert item[0] == reconstructed(item[2][0], b'x' * data['description_bytes'], item[3])
    for name, size in [('raw_over', data['description_bytes'] + 1),
                       ('base_tail', data['base_description']),
                       ('base_tail_over', data['base_description'] + 1)]:
        item = data[name]
        assert item[0] == reconstructed(item[2][0], b'x' * size, item[3])
    for item, desc, edges in zip(data['predecessors'], [b'pred-left', b'pred-right', b'pred-right'], [2047, 2047, 2048]):
        assert item[0] == reconstructed(item[2][0], desc, item[3], edges)
    expected_view = b''.join(field(1, i.to_bytes(8, 'little') + bytes(12)) for i in range(2048)) + bytes([96, 1])
    assert expected_view == data['shared'][0]
    for item, desc in zip(data['views'], [b'view-left', b'view-right', b'view-over']):
        assert item[0] == reconstructed(item[2][0], desc, item[3])
    merge = data['base']['MERGE']
    for key in ['RICH_PARENT', 'RICH_CHILD', 'LATE_BRANCH', 'MIXED_MERGE', 'CONVERGED_MERGE']:
        assert 'default' in decoded[data['small'][key][1]][1]['wc_commit_ids']
    def pred_count(items):
        return sum(sum(1 + len(edges) for edges in decoded[item[1]][0]['predecessors'].values()) for item in items)
    def view_count(items):
        return sum(decoded[item[1]][1]['commit_references'] for item in items)
    assert pred_count(data['predecessors'][:2]) == 4096
    assert pred_count([data['predecessors'][0], data['predecessors'][2]]) == 4097
    assert view_count(data['views'][:2]) == 4096
    assert view_count(data['views']) == 4097
    cases = {
        'chain_exact': (data['chain'][:256], [data['chain'][255][1]], 256),
        'chain_over': (data['chain'], [data['chain'][-1][1]], 257),
        'baseline_union_exact': (data['chain'][:255], [merge[1], data['chain'][254][1]], 256),
        'baseline_union_over': (data['chain'][:256], [merge[1], data['chain'][255][1]], 257),
        'raw_exact': (data['raw'], [data['raw'][-1][1]], 256),
        'raw_over': (data['raw'][:-1] + [data['raw_over']], [data['raw_over'][1]], 256),
        'raw_baseline_exact': (data['raw'][:254] + [data['base_tail']], [merge[1], data['base_tail'][1]], 256),
        'raw_baseline_over': (data['raw'][:254] + [data['base_tail_over']], [merge[1], data['base_tail_over'][1]], 256),
    }
    summaries = {}
    for name, (items, heads, expected_union) in cases.items():
        supplied = {item[1]: item for item in items}
        visited = set()
        queue = list(heads)
        while queue:
            identity = queue.pop()
            if identity == merge[1] or identity in visited:
                continue
            assert identity in supplied, (name, 'missing parent', identity)
            visited.add(identity)
            queue.extend(supplied[identity][2])
        assert visited == supplied.keys()
        union = len(items) + int(merge[1] in heads)
        assert union == expected_union
        raw = generator.raw_size(items) + (data['merge_bytes'] if merge[1] in heads else 0)
        if name.startswith('raw_'):
            assert raw == 8 * 1024 * 1024 + int(name.endswith('over'))
        assert view_count(items) <= 4096 and pred_count(items) <= 4096
        summaries[name] = dict(nonterminal_nodes=len(items), read_pair_union=union, union_raw_bytes=raw,
                               missing_parents=0, detached_nodes=0)
    report = dict(qualification='independent Python fixture calibration; no Rust test claim',
                  native_operation_hashes_verified=len(records), native_view_hashes_verified=len(views),
                  source_oracles={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in
                                  [oracle_dir / ('generate_' + name + '_vectors.py') for name in ['operation', 'view', 'evidence']]},
                  reader_sha256=hashlib.sha256(reader_path.read_bytes()).hexdigest(),
                  merge_pair_bytes=data['merge_bytes'], raw_description_bytes=data['description_bytes'],
                  raw_baseline_description_bytes=data['base_description'],
                  max_description_bytes=data['base_description'] + 1,
                  predecessor_counts=[4096, 4097], view_reference_counts=[4096, 4097], cases=summaries)
    output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'verified_operations': len(records), 'verified_views': len(views),
                      'cases': len(summaries), 'max_description_bytes': data['base_description'] + 1}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--oracle-dir', required=True, type=Path)
    parser.add_argument('--reader', required=True, type=Path)
    parser.add_argument('--output', type=Path, default=Path(__file__).with_name('calibration.json'))
    args = parser.parse_args()
    main(args.oracle_dir, args.reader, args.output)
