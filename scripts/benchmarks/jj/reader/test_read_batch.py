"""Real-repository acceptance tests for the research-only bounded jj reader."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

from decode_store import decode_operation
from probe_cli import CASES, Fixtures, ROOT, exact_history, manifest, remove_index
from read_batch import PROFILE, Limits, Reader, ReadError, read_batch


class RealJjReaderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix='git-ai-jj-reader-tests-')
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.root = Path(cls.temporary.name)
        cls.fixtures = Fixtures(cls.root)

    def read_unchanged(self, repo, *, working_copy=None, known=(), manifest_root=None, limits=Limits()):
        monitored = manifest_root or repo.parent.parent
        before = manifest(monitored)
        persisted = frozenset(known) | {ROOT}
        result = read_batch(repo, profile=PROFILE, is_known_operation=persisted.__contains__,
                            working_copy=working_copy, limits=limits)
        self.assertEqual(before, manifest(monitored))
        self.assertEqual(result['counters']['subprocesses'], 0)
        self.assertEqual(result['counters']['git_object_reads'], 0)
        self.assertLessEqual(result['counters']['operations'], limits.operations)
        self.assertLessEqual(result['counters']['views'], limits.views)
        self.assertLessEqual(result['counters']['predecessor_entries'], limits.predecessors)
        self.assertLessEqual(result['counters']['commit_references'], limits.commit_references)
        self.assertLessEqual(result['counters']['bytes'], limits.total_bytes)
        self.assertTrue(all(view['domain_hash_verified'] for view in result['views']))
        return result

    def test_complete_immutable_evidence_for_all_operation_families(self):
        for name in CASES:
            with self.subTest(operation=name):
                case = self.fixtures.case(name)
                repo = case['repo']/'.jj/repo'
                result = self.read_unchanged(repo, working_copy=case['repo']/'.jj/working_copy')
                operations = {operation['operation_id']: operation for operation in result['operations']}
                self.assertEqual(operations, exact_history(repo, case['heads']))
                self.assertEqual(result['sampled_heads'], case['heads'])
                self.assertFalse(set(case['detached']) & operations.keys())
                positions = {identity: n for n, identity in enumerate(operations)}
                for identity, operation in operations.items():
                    for parent in operation['parents']:
                        if parent != ROOT:
                            self.assertLess(positions[parent], positions[identity])
                self.assertTrue(result['checkout']['in_captured_history'])

    def test_durable_membership_returns_only_new_history(self):
        workspace = self.fixtures.prepare('cut')
        repo = workspace/'.jj/repo'
        first = self.read_unchanged(repo)
        known = frozenset(x['operation_id'] for x in first['operations']) | {ROOT}
        previous_head = first['sampled_heads'][0]
        self.fixtures.run(workspace, ['describe', '-r', '@-', '-m', 'after cut'])
        heads = self.fixtures.heads(workspace)
        result = self.read_unchanged(repo, known=known, working_copy=workspace/'.jj/working_copy')
        self.assertEqual({x['operation_id']: x for x in result['operations']},
                         exact_history(repo, heads, cut=known))
        self.assertNotIn(previous_head, [x['operation_id'] for x in result['operations']])
        self.assertEqual(result['known_boundaries'], [previous_head])

    def test_stale_additional_workspace_keeps_its_own_checkout_identity(self):
        workspace = self.fixtures.prepare('linked-main')
        second = self.root/'linked-second'
        self.fixtures.run(workspace, ['workspace', 'add', str(second)])
        repo = workspace/'.jj/repo'
        pointer = second/'.jj/repo'
        self.assertTrue(pointer.is_file())
        self.assertEqual((pointer.parent/pointer.read_text()).resolve(), repo.resolve())
        before = self.read_unchanged(repo, working_copy=second/'.jj/working_copy', manifest_root=self.root)
        self.fixtures.run(workspace, ['describe', '-r', '@-', '-m', 'rewrite with second stale'])
        after = self.read_unchanged(repo, working_copy=second/'.jj/working_copy', manifest_root=self.root)
        self.assertEqual(before['checkout'], after['checkout'])
        self.assertEqual(after['checkout']['workspace'], 'linked-second')
        self.assertNotEqual(after['sampled_heads'], before['sampled_heads'])
        self.assertNotIn(after['checkout']['operation_id'], after['sampled_heads'])
        self.assertTrue(after['checkout']['in_captured_history'])

    def test_redundant_ancestor_head_is_preserved_without_normalization(self):
        workspace = self.fixtures.prepare('redundant-head')
        repo = workspace/'.jj/repo'
        latest = self.fixtures.heads(workspace)[0]
        parent = decode_operation(repo/'op_store/operations'/latest)['parents'][0]
        marker = repo/'op_heads/heads'/parent
        marker.touch()
        result = self.read_unchanged(repo, working_copy=workspace/'.jj/working_copy')
        self.assertEqual(result['sampled_heads'], sorted([latest, parent]))
        self.assertTrue(marker.exists())
        self.assertEqual(len(result['operations']), len(exact_history(repo, [latest])))

    def test_redundant_heads_allow_noop_replay_from_durable_membership(self):
        workspace = self.fixtures.prepare('redundant-noop')
        repo = workspace/'.jj/repo'
        latest = self.fixtures.heads(workspace)[0]
        parent = decode_operation(repo/'op_store/operations'/latest)['parents'][0]
        (repo/'op_heads/heads'/parent).touch()
        first = self.read_unchanged(repo)
        known = frozenset(x['operation_id'] for x in first['operations']) | {ROOT}
        replay = self.read_unchanged(repo, known=known)
        self.assertEqual(replay['operations'], [])
        self.assertEqual(replay['sampled_heads'], first['sampled_heads'])
        self.assertEqual(replay['known_boundaries'], first['sampled_heads'])

    def test_redundant_heads_allow_successor_replay_from_durable_membership(self):
        workspace = self.fixtures.prepare('redundant-successor')
        repo = workspace/'.jj/repo'
        latest = self.fixtures.heads(workspace)[0]
        parent = decode_operation(repo/'op_store/operations'/latest)['parents'][0]
        (repo/'op_heads/heads'/parent).touch()
        first = self.read_unchanged(repo)
        known = frozenset(x['operation_id'] for x in first['operations']) | {ROOT}
        self.fixtures.run(workspace, ['--at-operation='+latest, 'describe', '-r', '@-', '-m', 'successor'])
        replay = self.read_unchanged(repo, known=known)
        self.assertIn(latest, replay['known_boundaries'])
        self.assertEqual(len(replay['operations']), 1)
        self.assertNotIn(parent, [x['operation_id'] for x in replay['operations']])

    def test_late_concurrent_branch_stops_at_its_own_durable_ancestor(self):
        workspace = self.fixtures.prepare('late-concurrent')
        repo = workspace/'.jj/repo'
        ancestor = self.fixtures.heads(workspace)[0]
        self.fixtures.run(workspace, ['describe', '-r', '@-', '-m', 'already observed'])
        first = self.read_unchanged(repo)
        known = frozenset(x['operation_id'] for x in first['operations']) | {ROOT}
        self.fixtures.run(workspace, ['--at-operation='+ancestor, 'describe', '-r', '@-', '-m', 'late concurrent'])
        replay = self.read_unchanged(repo, known=known)
        self.assertEqual(len(replay['sampled_heads']), 2)
        self.assertEqual(len(replay['operations']), 1)
        self.assertEqual(replay['operations'][0]['parents'], [ancestor])
        self.assertEqual(set(replay['known_boundaries']), {ancestor, first['sampled_heads'][0]})

    def test_missing_commit_index_is_never_rebuilt(self):
        workspace = self.fixtures.prepare('missing-index-reader')
        repo = workspace/'.jj/repo'
        remove_index(workspace)
        result = self.read_unchanged(repo, working_copy=workspace/'.jj/working_copy')
        self.assertGreater(len(result['operations']), 0)
        self.assertEqual([p.name for p in (repo/'index').iterdir()], ['type'])

    def test_overflow_and_missing_history_fail_without_partial_success(self):
        workspace = self.fixtures.prepare('budget')
        repo = workspace/'.jj/repo'
        before = manifest(workspace)
        for limits in (Limits(operations=1), Limits(views=1), Limits(predecessors=1), Limits(commit_references=1),
                       Limits(record_bytes=1), Limits(total_bytes=1)):
            with self.subTest(limits=limits):
                with self.assertRaises(ReadError):
                    read_batch(repo, profile=PROFILE, limits=limits)
                self.assertEqual(before, manifest(workspace))
        with self.assertRaises(ReadError):
            read_batch(repo, profile='unknown-profile')
        self.assertEqual(before, manifest(workspace))

    def test_head_and_parent_fan_in_budgets_are_enforced(self):
        workspace = self.fixtures.prepare('fan-in')
        repo = workspace/'.jj/repo'
        base = self.fixtures.heads(workspace)[0]
        for description in ('one', 'two'):
            self.fixtures.run(workspace, ['--at-operation='+base, 'describe', '-r', '@-', '-m', description])
        before = manifest(workspace)
        with self.assertRaisesRegex(ReadError, 'head_budget'):
            read_batch(repo, profile=PROFILE, limits=Limits(heads=1))
        self.assertEqual(before, manifest(workspace))
        self.fixtures.run(workspace, ['status'])
        before = manifest(workspace)
        with self.assertRaisesRegex(ReadError, 'parent_budget'):
            read_batch(repo, profile=PROFILE, limits=Limits(heads=1))
        self.assertEqual(before, manifest(workspace))

    def test_elapsed_budget_is_deterministic_and_nonmutating(self):
        workspace = self.fixtures.prepare('elapsed')
        before = manifest(workspace)
        with patch('read_batch.time.monotonic', side_effect=[0.0, 1.0]):
            with self.assertRaisesRegex(ReadError, 'elapsed_budget'):
                read_batch(workspace/'.jj/repo', profile=PROFILE)
        self.assertEqual(before, manifest(workspace))

    def test_unrelated_membership_never_truncates_reachable_history(self):
        workspace = self.fixtures.prepare('unrelated-membership')
        repo = workspace/'.jj/repo'
        result = self.read_unchanged(repo, known={'f'*128})
        self.assertEqual({x['operation_id']: x for x in result['operations']},
                         exact_history(repo, self.fixtures.heads(workspace)))
        self.assertEqual(result['known_boundaries'], [ROOT])

    def test_membership_is_queried_once_per_operation(self):
        workspace = self.fixtures.prepare('membership-cache')
        repo = workspace/'.jj/repo'
        calls = {}

        def membership(identity):
            calls[identity] = calls.get(identity, 0)+1
            return False

        before = manifest(workspace)
        result = read_batch(repo, profile=PROFILE, is_known_operation=membership)
        self.assertEqual(before, manifest(workspace))
        self.assertEqual(set(calls), {x['operation_id'] for x in result['operations']})
        self.assertEqual(set(calls.values()), {1})

    def test_head_markers_reject_directories_symlinks_content_and_unknown_names(self):
        workspace = self.fixtures.prepare('markers')
        repo = workspace/'.jj/repo'
        marker = repo/'op_heads/heads'/('f'*128)
        current = repo/'op_heads/heads'/self.fixtures.heads(workspace)[0]
        for kind in ('directory', 'symlink', 'nonempty'):
            with self.subTest(kind=kind):
                if kind == 'directory':
                    marker.mkdir()
                elif kind == 'symlink':
                    marker.symlink_to(current)
                else:
                    marker.write_bytes(b'not an empty head marker')
                before = manifest(workspace)
                with self.assertRaisesRegex(ReadError, 'invalid_head_marker'):
                    read_batch(repo, profile=PROFILE)
                self.assertEqual(before, manifest(workspace))
                if kind == 'directory':
                    marker.rmdir()
                else:
                    marker.unlink()
        marker = repo/'op_heads/heads'/'invalid-name'
        marker.touch()
        before = manifest(workspace)
        with self.assertRaisesRegex(ReadError, 'invalid_operation_id'):
            read_batch(repo, profile=PROFILE)
        self.assertEqual(before, manifest(workspace))

    def test_missing_history_and_malformed_checkout_are_rejected(self):
        workspace = self.fixtures.prepare('missing-history')
        repo = workspace/'.jj/repo'
        head = self.fixtures.heads(workspace)[0]
        operation_path = repo/'op_store/operations'/head
        operation_bytes = operation_path.read_bytes()
        operation_path.unlink()
        before = manifest(workspace)
        with self.assertRaises(FileNotFoundError):
            read_batch(repo, profile=PROFILE)
        self.assertEqual(before, manifest(workspace))
        operation_path.write_bytes(operation_bytes)
        checkout = workspace/'.jj/working_copy/checkout'
        checkout.write_bytes(b'\x12\x01\x00')
        before = manifest(workspace)
        with self.assertRaisesRegex(ValueError, 'wrong id length'):
            read_batch(repo, profile=PROFILE, working_copy=checkout.parent)
        self.assertEqual(before, manifest(workspace))

    def test_unknown_working_copy_backend_is_rejected(self):
        workspace = self.fixtures.prepare('unknown-working-copy')
        working_copy = workspace/'.jj/working_copy'
        (working_copy/'type').write_bytes(b'unknown_provider')
        before = manifest(workspace)
        with self.assertRaisesRegex(ReadError, 'unsupported_backend'):
            read_batch(workspace/'.jj/repo', profile=PROFILE, working_copy=working_copy)
        self.assertEqual(before, manifest(workspace))

    def test_changed_head_sample_is_rejected(self):
        workspace = self.fixtures.prepare('head-race')
        repo = workspace/'.jj/repo'
        original = Reader.heads
        calls = 0

        def changing_sample(reader):
            nonlocal calls
            calls += 1
            result = original(reader)
            return result if calls == 1 else sorted(result+['f'*128])

        before = manifest(workspace)
        with patch.object(Reader, 'heads', changing_sample):
            with self.assertRaisesRegex(ReadError, 'heads_changed'):
                read_batch(repo, profile=PROFILE)
        self.assertEqual(before, manifest(workspace))

    def test_changed_checkout_sample_is_rejected(self):
        workspace = self.fixtures.prepare('checkout-race')
        repo = workspace/'.jj/repo'
        original = Reader.read
        checkout_reads = 0

        def changing_sample(reader, path):
            nonlocal checkout_reads
            result = original(reader, path)
            if path.name == 'checkout':
                checkout_reads += 1
                if checkout_reads == 2:
                    return result+b'\x00'
            return result

        before = manifest(workspace)
        with patch.object(Reader, 'read', changing_sample):
            with self.assertRaisesRegex(ReadError, 'checkout_changed'):
                read_batch(repo, profile=PROFILE, working_copy=workspace/'.jj/working_copy')
        self.assertEqual(before, manifest(workspace))


if __name__ == '__main__':
    unittest.main()
