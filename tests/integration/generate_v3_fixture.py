#!/usr/bin/env python3
"""Append independently encoded registrations to the exact frozen v2 fixture.

Synthetic locator/inode metadata is a stored-format fixture, not physical source
continuity. Existing native/opaque rows are copied verbatim and not regenerated.
"""
import argparse
import hashlib
import json
import sqlite3
import sys
from pathlib import Path

sys.dont_write_bytecode = True

REGISTRATIONS = """CREATE TABLE jj_native_registrations (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    source_root_key TEXT NOT NULL UNIQUE,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);"""
WORKSPACES = """CREATE TABLE jj_native_workspaces (
    source_id TEXT NOT NULL REFERENCES jj_native_registrations(source_id),
    workspace_name TEXT NOT NULL,
    locator_key TEXT NOT NULL,
    workspace_root_key TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, workspace_name),
    UNIQUE (locator_key),
    UNIQUE (source_id, workspace_root_key)
);"""


def generate(oracle_dir, frozen_v2):
    sys.path.insert(0, str(oracle_dir))
    from generate_evidence_vectors import pairs
    from generate_registration_records import BASELINE, PROFILE, cbor, digest
    from generate_registration_records import guard_preimages, source_record, workspace

    conn = sqlite3.connect(":memory:")
    conn.executescript(frozen_v2.decode("utf-8"))
    assert conn.execute("SELECT value FROM schema_metadata WHERE key='version'").fetchone() == ('2',)
    rows = conn.execute("SELECT source_id,baseline_id FROM jj_native_sources").fetchall()
    assert len(rows) == 1
    source, baseline = rows[0]
    assert source == f"{1:064x}" and baseline == BASELINE
    native = pairs()
    first, _ = native['FIRST']
    merge, rich = native['MERGE']
    name = "default"
    seal = (f"git-ai/jj/source-seal/v1\nsource_id={source}\nreader_profile={PROFILE}\n").encode()
    selected = workspace(platform="linux", source=1, name=name, number=201)
    selected['seal_digest'] = digest(seal)
    # The MERGE checkout is deliberately outside the saved FIRST cutoff. Its
    # bytes are valid, but neither this record nor its synthetic path is live.
    selected['selected_checkout'] = {
        'raw_checkout_bytes': bytes([0x12,64]) + bytes.fromhex(merge[1]) + bytes([0x1a,len(name)]) + name.encode(),
        'operation_id': merge[1],
        'view_id': rich[1],
        'baseline_relation': 'outside_baseline',
    }
    registration = source_record(selected, seal)
    registration['source_binding']['backends'] = [b'git', b'simple_op_store', b'simple_op_heads_store']
    source_bytes, workspace_bytes = cbor(registration), cbor(selected)
    source_hash, workspace_hash = digest(source_bytes), digest(workspace_bytes)
    source_guard = {k:digest(v) for k,v in guard_preimages('source',registration).items()}
    workspace_guards = {k:digest(v) for k,v in guard_preimages('workspace',selected).items()}
    assert registration['initial_workspace_record_id'] == workspace_hash
    suffix = f"""
-- Frozen v3 extension: structural registration of the saved native FIRST cutoff.
-- Paths and Unix identities below are synthetic; no current-source claim.
{REGISTRATIONS}
{WORKSPACES}
UPDATE schema_metadata SET value = '3' WHERE key = 'version';
INSERT INTO jj_native_registrations(source_id,baseline_id,source_root_key,record,checksum)
VALUES ('{source}','{baseline}','{source_guard['source_root_key']}',X'{source_bytes.hex()}','{source_hash}');
INSERT INTO jj_native_workspaces(source_id,workspace_name,locator_key,workspace_root_key,record,checksum)
VALUES ('{source}','{name}','{workspace_guards['locator_key']}','{workspace_guards['workspace_root_key']}',X'{workspace_bytes.hex()}','{workspace_hash}');
""".encode()
    result = frozen_v2 + suffix
    assert result.startswith(frozen_v2)
    metadata = {
        'schema':'git-ai/jj/frozen-schema3-fixture/v1',
        'source_id':source,'workspace_name':name,'baseline_id':baseline,
        'baseline_head_id':first[1],'checkout_operation_id':merge[1],
        'checkout_view_id':rich[1],'reader_profile':PROFILE,
        'registration_checksum':source_hash,'workspace_checksum':workspace_hash,
        'registration_bytes':len(source_bytes),'workspace_bytes':len(workspace_bytes),
        'seal_bytes':len(seal),**source_guard,**workspace_guards,
        'frozen_v2_sha256':digest(frozen_v2),'frozen_v2_bytes':len(frozen_v2),
        'sql_sha256':digest(result),
        'oracle_sha256':{name:hashlib.sha256((oracle_dir/name).read_bytes()).hexdigest() for name in [
            'generate_registration_records.py','generate_evidence_vectors.py',
            'generate_operation_vectors.py','generate_view_vectors.py']},
        'claim':'native-valid original baseline and canonical stored registration; no current physical binding',
    }
    return result,metadata


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--oracle-dir',required=True,type=Path)
    parser.add_argument('--v2',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--metadata',required=True,type=Path)
    args=parser.parse_args()
    sql,metadata=generate(args.oracle_dir,args.v2.read_bytes())
    args.output.write_bytes(sql)
    args.metadata.write_text(json.dumps(metadata,indent=2)+'\n')
    print(json.dumps({'source_id':metadata['source_id'],'workspace_name':metadata['workspace_name'],
                     'baseline_id':metadata['baseline_id'],'sql_sha256':metadata['sql_sha256']}))
