#!/usr/bin/env python3
"""Compare pinned jj CLI observation with exact immutable-store evidence.

Requires explicit GIT_AI_TEST_JJ_BINARY and GIT_AI_TEST_REAL_GIT paths. This qualification
probe creates and removes only temporary repositories and isolated configuration.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True

from decode_store import decode_operation

PINNED_VERSION = 'jj 0.45.1-7c41cdeb16b6b321c64e789a966b6adf723816a5'
ROOT = '0' * 128
CASES = ('snapshot', 'new', 'commit', 'describe', 'rebase', 'split', 'squash',
         'absorb', 'abandon', 'undo', 'restore', 'import', 'divergence', 'detached')
OP = ('"{\\"id\\":" ++ json(id) ++ ",\\"parents\\":" ++ '
      'json(parents.map(|p| p.id())) ++ ",\\"snapshot\\":" ++ json(snapshot) ++ "}\\n"')
REV = ('"{\\"commit\\":" ++ json(commit_id) ++ ",\\"change\\":" ++ '
       'json(change_id) ++ ",\\"parents\\":" ++ json(parents.map(|p| p.commit_id())) ++ "}\\n"')
EVO = ('"{\\"commit\\":" ++ json(commit.commit_id()) ++ ",\\"operation\\":" ++ '
       'if(operation, json(operation.id()), "null") ++ ",\\"predecessors\\":" ++ '
       'json(predecessors.map(|p| p.commit_id())) ++ "}\\n"')
WORKSPACE = ('"{\\"name\\":" ++ json(name) ++ ",\\"commit\\":" ++ '
             'json(target.commit_id()) ++ "}\\n"')


def required_binary(name):
    value = os.environ.get(name)
    if not value or not Path(value).is_absolute() or not Path(value).is_file() or not os.access(value, os.X_OK):
        raise ValueError(f'{name} must name an explicit absolute executable file')
    return Path(value)


def manifest(root):
    result = {}
    for path in [root, *sorted(root.rglob('*'))]:
        info = path.lstat()
        value = [info.st_mode, info.st_size, info.st_ino,
                 info.st_mtime_ns, info.st_ctime_ns]
        if stat.S_ISREG(info.st_mode):
            value.append(hashlib.sha256(path.read_bytes()).hexdigest())
        elif stat.S_ISLNK(info.st_mode):
            value.append(os.readlink(path))
        result[str(path.relative_to(root))] = value
    return result


def remove_index(repo):
    for path in (repo/'.jj/repo/index').iterdir():
        if path.name != 'type':
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()


class Fixtures:
    def __init__(self, root):
        self.root = root
        self.jj_binary = required_binary('GIT_AI_TEST_JJ_BINARY')
        self.git_binary = required_binary('GIT_AI_TEST_REAL_GIT')
        isolated_home = root/'home'
        isolated_home.mkdir()
        config = root/'config.toml'
        config.write_text('[user]\nname="Reader Probe"\nemail="reader@example.invalid"\n'
                          '[ui]\npager="cat"\neditor="true"\n')
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(('GIT_', 'JJ_', 'XDG_'))
                    and key not in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA')}
        for key, directory in (('HOME', 'home'), ('USERPROFILE', 'home'),
                               ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                               ('XDG_DATA_HOME', 'data'), ('XDG_STATE_HOME', 'state'),
                               ('APPDATA', 'appdata'), ('LOCALAPPDATA', 'localappdata')):
            path = root/directory
            path.mkdir(exist_ok=True)
            self.env[key] = str(path)
        empty_git_config = root/'empty-git-config'
        empty_git_config.write_text('')
        self.env.update(JJ_CONFIG=str(config), GIT_CONFIG_NOSYSTEM='1',
                        GIT_CONFIG_GLOBAL=str(empty_git_config), GIT_AUTHOR_NAME='Reader Probe',
                        GIT_AUTHOR_EMAIL='reader@example.invalid',
                        GIT_COMMITTER_NAME='Reader Probe',
                        GIT_COMMITTER_EMAIL='reader@example.invalid')
        actual = self.run(root, ['version']).stdout.strip()
        if actual != PINNED_VERSION:
            raise ValueError('jj version differs from the qualified pinned build')

    def run(self, repo, args, *, git=False, allow_failure=False):
        command = [str(self.git_binary if git else self.jj_binary)]
        if not git:
            command += ['--no-pager', '--color=never']
        result = subprocess.run(command+args, cwd=repo, env=self.env,
                                capture_output=True, text=True, timeout=30)
        if result.returncode and not allow_failure:
            raise ValueError(f'fixture command failed (exit {result.returncode})')
        return result

    def heads(self, repo):
        return sorted(path.name for path in (repo/'.jj/repo/op_heads/heads').iterdir()
                      if path.is_file() and path.name != 'lock')

    def observe(self, repo, operation, args):
        before = manifest(repo)
        started = time.monotonic()
        result = self.run(repo, ['--ignore-working-copy', '--at-operation='+operation]+args,
                          allow_failure=True)
        elapsed_ms = (time.monotonic()-started)*1000
        after = manifest(repo)
        changed = sorted(key for key in before.keys() | after.keys()
                         if before.get(key) != after.get(key))
        return dict(exit_code=result.returncode, elapsed_ms=elapsed_ms,
                    manifest_unchanged=before == after, changed_paths=changed,
                    rows=[json.loads(line) for line in result.stdout.splitlines()]
                    if result.returncode == 0 else [])

    def prepare(self, name, *, colocated=False):
        repo = self.root/name
        self.run(self.root, ['git', 'init', '--colocate' if colocated else '--no-colocate', str(repo)])
        (repo/'a').write_text('alpha\nbeta\ngamma\n')
        (repo/'b').write_text('one\ntwo\nthree\n')
        self.run(repo, ['commit', '-m', 'base'])
        (repo/'a').write_text('alpha\nbeta middle\ngamma\n')
        (repo/'b').write_text('one\ntwo middle\nthree\n')
        self.run(repo, ['commit', '-m', 'middle'])
        return repo

    def case(self, name):
        repo = self.prepare(name, colocated=(name == 'import'))
        before = self.heads(repo)[0]
        initial = self.observe(repo, before, ['op', 'log', '-G', '-n', '256', '-T', OP])
        if name in ('snapshot', 'commit', 'absorb'):
            (repo/'a').write_text('alpha\nbeta absorbed\ngamma\n')
        commands = {
            'snapshot': ['status'], 'new': ['new'], 'commit': ['commit', '-m', 'final'],
            'describe': ['describe', '-r', '@-', '-m', 'middle renamed'],
            'rebase': ['rebase', '-r', '@-', '-d', 'root()'],
            'split': ['split', '-r', '@-', 'a', '-m', 'split a'],
            'squash': ['squash', '--from', '@-', '--into', '@--', '-m', 'squashed'],
            'absorb': ['absorb'], 'abandon': ['abandon', '-r', '@-'], 'undo': ['undo'],
            'restore': ['op', 'restore', initial['rows'][-2]['id']],
            'import': ['git', 'import'],
            'divergence': ['--at-operation='+before, 'describe', '-r', '@-', '-m', 'parallel right'],
            'detached': ['--no-integrate-operation', 'describe', '-r', '@-', '-m', 'detached'],
        }
        if name == 'import':
            (repo/'c').write_text('external git line\n')
            self.run(repo, ['add', 'c'], git=True)
            self.run(repo, ['commit', '-m', 'external import'], git=True)
        if name == 'divergence':
            self.run(repo, ['--at-operation='+before, 'describe', '-r', '@-', '-m', 'parallel left'])
        mutation = self.run(repo, commands[name])
        detached = re.findall(r'\b[0-9a-f]{128}\b', mutation.stdout+mutation.stderr) if name == 'detached' else []
        (repo/'observer-dirty').write_text('not snapshotted\n')
        return dict(repo=repo, before=before, heads=self.heads(repo), detached=detached)


def exact_history(repo, heads, cut=(ROOT,)):
    result, pending = {}, list(heads)
    while pending:
        identity = pending.pop()
        if identity in cut or identity in result:
            continue
        operation = decode_operation(repo/'op_store/operations'/identity)
        result[identity] = operation
        pending.extend(operation['parents'])
    return result


def compare_mappings(expected, rows):
    actual = {(row['operation'], row['commit']): row['predecessors']
              for row in rows if row['operation']}
    return dict(expected=len(expected), observed=len(actual),
                missing=sum(key not in actual for key in expected),
                mismatched=sum(actual[key] != value for key, value in expected.items()
                               if key in actual))


def probe(root):
    fixtures = Fixtures(root)
    report = dict(version=PINNED_VERSION,
                  jj_sha256=hashlib.sha256(fixtures.jj_binary.read_bytes()).hexdigest(), cases={})
    for name in CASES:
        case = fixtures.case(name)
        head_reports = []
        for head in case['heads']:
            records = {}
            for label, args in [('operations', ['op', 'log', '-G', '-n', '256', '-T', OP]),
                                ('revisions', ['log', '-G', '-r', 'all()', '-n', '256', '-T', REV]),
                                ('evolution', ['evolog', '-G', '-r', 'all()', '-n', '256', '-T', EVO]),
                                ('workspaces', ['workspace', 'list', '-T', WORKSPACE])]:
                records[label] = fixtures.observe(case['repo'], head, args)
            raw = exact_history(case['repo']/'.jj/repo', [head])
            expected = {(identity, commit): predecessors for identity, operation in raw.items()
                        for commit, predecessors in operation['predecessors'].items()}
            comparison = compare_mappings(expected, records['evolution']['rows'])
            summary = dict(spawn_count=4, raw_operation_count=len(raw), comparison=comparison,
                           observations={key: {k: v for k, v in value.items() if k != 'rows'}
                                         for key, value in records.items()})
            if comparison['missing']:
                union = ' | '.join('at_operation('+identity+', all())' for identity in raw)
                outcome = fixtures.observe(case['repo'], head,
                                           ['evolog', '-G', '-r', union, '-n', '256', '-T', EVO])
                summary['union_comparison'] = compare_mappings(expected, outcome.pop('rows'))
                summary['union_observation'] = outcome
                summary['spawn_count'] += 1
            if name == 'detached':
                summary['detached_excluded'] = not bool(set(case['detached']) & set(raw))
            head_reports.append(summary)
        report['cases'][name] = head_reports
    repo = fixtures.prepare('missing-index')
    head = fixtures.heads(repo)[0]
    remove_index(repo)
    counterexample = fixtures.observe(repo, head, ['evolog', '-G', '-r', '@', '-n', '1', '-T', EVO])
    counterexample.pop('rows')
    report['missing_index_evolog'] = counterexample
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='New sanitized JSON evidence file')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output file must be new')
    with tempfile.TemporaryDirectory(prefix='git-ai-jj-cli-proof-') as temporary:
        result = probe(Path(temporary))
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
        output.write('\n')


if __name__ == '__main__':
    main()
