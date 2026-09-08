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

import hashlib
import os
import subprocess
import tempfile
import unittest
import sys
from pathlib import Path
sys.dont_write_bytecode = True

import decode_store as d

def manifest(repo):
    return {
        str(p.relative_to(repo)): (
            p.stat().st_size,
            p.stat().st_mtime_ns,
            hashlib.sha256(p.read_bytes()).hexdigest(),
        )
        for p in repo.rglob('*')
        if p.is_file() and not p.is_symlink()
    }

class DecoderProof(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        paths = {}
        for key in ('GIT_AI_TEST_JJ_BINARY', 'GIT_AI_TEST_REAL_GIT'):
            raw = os.environ.get(key)
            if not raw:
                raise RuntimeError(f'{key} must identify the explicit test binary')
            path = Path(raw)
            if not path.is_absolute() or not path.is_file() or (not os.access(path, os.X_OK)):
                raise RuntimeError(f'{key} must be an absolute executable file')
            paths[key] = str(path)
        cls.jj_binary = paths['GIT_AI_TEST_JJ_BINARY']
        cls.git_binary = paths['GIT_AI_TEST_REAL_GIT']

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='jj-raw-reader-')
        self.addCleanup(self.temp.cleanup)
        self.p = Path(self.temp.name)
        self.case = self.p / 'repo'
        self.case.mkdir()
        cfg = self.p / 'config.toml'
        cfg.write_text('[user]\nname="Reader Probe"\nemail="reader@example.invalid"\n[ui]\npager="never"\n')
        self.env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(('GIT_', 'JJ_', 'XDG_'))
            and key not in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA')
        }
        for key, directory in (
            ('HOME', 'home'),
            ('USERPROFILE', 'home'),
            ('XDG_CONFIG_HOME', 'config'),
            ('XDG_CACHE_HOME', 'cache'),
            ('XDG_DATA_HOME', 'data'),
            ('XDG_STATE_HOME', 'state'),
            ('APPDATA', 'appdata'),
            ('LOCALAPPDATA', 'localappdata'),
        ):
            path = self.p / directory
            path.mkdir(exist_ok=True)
            self.env[key] = str(path)
        empty_git_config = self.p / 'empty-git-config'
        empty_git_config.write_text('')
        self.env.update(JJ_CONFIG=str(cfg), GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(empty_git_config))
        self.assertIn(self.jj('--version'), ('jj 0.45.1', 'jj 0.45.1-7c41cdeb16b6b321c64e789a966b6adf723816a5'))
        self.jj('git', 'init', '--colocate')
        (self.case / 'a').write_text('alpha\n')
        (self.case / 'b').write_text('bravo\n')
        self.jj('commit', '-m', 'initial')

    def tearDown(self):
        self.temp.cleanup()

    def jj(self, *args, cwd=None):
        r = subprocess.run(
            [self.jj_binary, '--no-pager', *args],
            cwd=cwd or self.case,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def git(self, *args):
        r = subprocess.run(
            [self.git_binary, *args],
            cwd=self.case,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def check_all(self, label):
        before = manifest(self.case)
        repo = self.case / '.jj' / 'repo'
        ops = [d.decode_operation(p) for p in (repo / 'op_store' / 'operations').iterdir() if len(p.name) == 128]
        views = [d.decode_view(p) for p in (repo / 'op_store' / 'views').iterdir() if len(p.name) == 128]
        self.assertEqual(before, manifest(self.case))
        self.assertTrue(all((x['domain_hash_verified'] for x in ops + views)))
        heads = [p.name for p in (repo / 'op_heads' / 'heads').iterdir() if len(p.name) == 128]
        headops = [o for o in ops if o['operation_id'] in heads]
        return (ops, views, headops)

    def test_split(self):
        self.jj('split', '-r', '@-', 'a', '-m', 'selected')
        _, _, ops = self.check_all('split')
        values = list(ops[0]['predecessors'].values())
        self.assertTrue(any((values.count(v) >= 2 for v in values if len(v) == 1)))

    def test_squash(self):
        (self.case / 'a').write_text('alpha\nadditional\n')
        self.jj('describe', '-m', 'source')
        self.jj('squash', '-m', 'squashed')
        _, _, ops = self.check_all('squash')
        self.assertTrue(any((len(v) >= 2 for v in ops[0]['predecessors'].values())))

    def test_absorb(self):
        (self.case / 'a').write_text('alpha changed\n')
        self.jj('describe', '-m', 'source')
        self.jj('absorb')
        _, _, ops = self.check_all('absorb')
        self.assertTrue(any((len(v) >= 2 for v in ops[0]['predecessors'].values())))

    def test_undo_restore(self):
        self.jj('describe', '-m', 'described')
        _, _, old = self.check_all('before_undo')
        self.jj('undo')
        _, _, ops = self.check_all('undo')
        self.assertEqual(ops[0]['predecessors'], {})
        self.assertTrue(ops[0]['stores_commit_predecessors'])
        self.jj('op', 'restore', old[0]['operation_id'])
        _, _, restored = self.check_all('op_restore')
        self.assertEqual(restored[0]['predecessors'], {})

    def test_refs_remotes_and_workspaces(self):
        self.jj('bookmark', 'create', 'main', '-r', '@-')
        self.jj('tag', 'set', 'v1', '-r', '@-')
        self.jj('git', 'export')
        self.git('update-ref', 'refs/remotes/origin/main', 'HEAD')
        self.git('config', 'remote.origin.url', str(self.case))
        self.jj('git', 'import')
        self.jj('bookmark', 'track', 'main@origin')
        self.jj('workspace', 'add', str(self.p / 'linked'), '--name', 'linked')
        _, views, _ = self.check_all('refs_remotes_workspace')
        self.assertTrue(any((v['counts']['remotes'] for v in views)))
        self.assertTrue(any((v['counts']['tags'] for v in views)))
        self.assertTrue(any((len(v['wc_commit_ids']) == 2 for v in views)))

    def test_git_import_existing_commit(self):
        (self.case / 'imported').write_text('external git\n')
        self.git('add', '.')
        self.git('-c', 'user.name=Reader', '-c', 'user.email=reader@example.invalid', 'commit', '-m', 'external')
        oid = self.git('rev-parse', 'HEAD')
        self.jj('git', 'import')
        all_operations, _, heads = self.check_all('git_import')
        self.assertTrue(all((oid not in operation['predecessors'] for operation in all_operations)))
        self.assertEqual(len(heads), 1)
        self.assertEqual(self.jj('--at-op', heads[0]['operation_id'], 'log', '-G',
                                 '-r', f'commit_id({oid}) & visible()', '-T', 'commit_id'), oid)

    def test_conflicting_bookmark(self):
        self.jj('bookmark', 'create', 'conflict', '-r', '@-')
        _, _, ops = self.check_all('before_divergence')
        base = ops[0]['operation_id']
        self.jj('--at-op', base, 'new', '-m', 'left')
        self.jj('bookmark', 'set', 'conflict', '-r', '@')
        self.jj('--at-op', base, 'new', '-m', 'right')
        _, _, heads = self.check_all('divergent_heads')
        right = next((o['operation_id'] for o in heads if o['parents'] == [base]))
        self.jj('--at-op', right, 'bookmark', 'set', 'conflict', '-r', '@')
        self.jj('--ignore-working-copy', 'log', '-r', 'none()')
        _, views, _ = self.check_all('concurrent_merge')
        self.assertTrue(any((v['counts']['conflicted_refs'] > 0 for v in views)))

    def test_malformed_and_hash_mismatch(self):
        repo = self.case / '.jj' / 'repo'
        op = next((p for p in (repo / 'op_store' / 'operations').iterdir() if len(p.name) == 128))
        view = next((p for p in (repo / 'op_store' / 'views').iterdir() if len(p.name) == 128))
        data = op.read_bytes()
        vd = view.read_bytes()
        with self.assertRaisesRegex(ValueError, 'unknown/wrong field'):
            d.decode_operation(op, data=data + b'0\x00')
        with self.assertRaisesRegex(ValueError, 'truncated'):
            d.decode_operation(op, data=data[:-1])
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            d.decode_operation(op.with_name('f' * 128), data=data)
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            d.decode_view(view.with_name('f' * 128), data=vd)
        with self.assertRaisesRegex(ValueError, 'unknown/wrong field'):
            d.decode_view(view, data=vd + b'p\x00')
        with self.assertRaisesRegex(ValueError, 'oversize'):
            d.decode_operation(op, data=b'x' * (d.MAX + 1))
        with self.assertRaisesRegex(ValueError, 'duplicate bookmark'):
            duplicate_absent = b'\x2a\x03\x0a\x01x' * 2
            d.decode_view(view, data=vd + duplicate_absent)
        with self.assertRaisesRegex(ValueError, 'legacy view'):
            d.decode_view(view, data=vd + b'\x12\x01x')
if __name__ == '__main__':
    unittest.main(verbosity=2)
