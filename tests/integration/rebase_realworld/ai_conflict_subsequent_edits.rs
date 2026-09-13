use super::*;

/// Test 7: dispatcher.py — conflict on C2.  C3 and C4 also modify dispatcher.py
/// (no further conflicts).  AI resolves C2 with 12-line process() implementation.
/// Subsequent commits append more methods to dispatcher.py.
#[test]
fn test_conflict_ai_resolves_then_more_ai_builds_on_result() {
    run_test_conflict_ai_resolves_then_more_ai_builds_on_result(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_then_more_ai_builds_on_result_standard_human() {
    run_test_conflict_ai_resolves_then_more_ai_builds_on_result(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_then_more_ai_builds_on_result(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: dispatcher.py stub (human)
    repo.commit_untracked_file(
        "dispatcher.py",
        "class Dispatcher:\n    pass\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: human implements process() differently → will conflict with feature's C2
    repo.commit_untracked_file(
        "dispatcher.py",
        "class Dispatcher:\n    def process(self, msg): return msg.strip()\n",
        "main: implement process() simply",
    );
    repo.commit_untracked_file(
        "config.py",
        "WORKERS = 4\nQUEUE_SIZE = 100\n",
        "main: add config",
    );
    repo.commit_untracked_file(
        "queue.py",
        "import queue\nQ = queue.Queue()\n",
        "main: add queue",
    );
    repo.commit_untracked_file(
        "worker.py",
        "class Worker:\n    def __init__(self, q): self.q = q\n",
        "main: add worker",
    );
    repo.commit_untracked_file(
        "monitor.py",
        "class Monitor:\n    def check(self): return 'ok'\n",
        "main: add monitor",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates base_handler.py (8 AI lines)
    let mut base_handler = repo.filename("base_handler.py");
    base_handler.set_contents(crate::lines![
        "class BaseHandler:".ai(),
        "    def __init__(self):".ai(),
        "        self.middlewares = []".ai(),
        "    def use(self, middleware):".ai(),
        "        self.middlewares.append(middleware)".ai(),
        "        return self".ai(),
        "    def handle(self, msg): raise NotImplementedError".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add BaseHandler")
        .unwrap();

    // C2: AI adds process() to dispatcher.py — WILL CONFLICT
    let mut dispatcher_c2 = repo.filename("dispatcher.py");
    dispatcher_c2.set_contents(crate::lines![
        human_context.expected_line("class Dispatcher:"),
        "    def process(self, msg):".ai(),
        "        msg = msg.strip()".ai(),
        "        if not msg: raise ValueError('empty')".ai(),
        "        tokens = msg.split()".ai(),
        "        return {'cmd': tokens[0], 'args': tokens[1:]}".ai(),
        human_context.expected_line("    pass"),
    ]);
    repo.stage_all_and_commit("feat: C2 AI adds process() to Dispatcher")
        .unwrap();

    // C3: AI creates router.py (does NOT touch dispatcher.py — no conflict)
    let mut router = repo.filename("router.py");
    router.set_contents(crate::lines![
        "from dispatcher import Dispatcher".ai(),
        "".ai(),
        "class Router:".ai(),
        "    def __init__(self):".ai(),
        "        self.dispatcher = Dispatcher()".ai(),
        "    def register(self, cmd, fn): self.dispatcher.route(cmd, fn)".ai(),
        "    def run(self, msg): return self.dispatcher.dispatch(msg)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 AI adds Router")
        .unwrap();

    // C4: AI creates middleware.py (new file, no conflict)
    let mut mw = repo.filename("middleware.py");
    mw.set_contents(crate::lines![
        "class Middleware:".ai(),
        "    def __init__(self): self.chain = []".ai(),
        "    def use(self, fn): self.chain.append(fn); return self".ai(),
        "    def run(self, msg):".ai(),
        "        for fn in self.chain: msg = fn(msg)".ai(),
        "        return msg".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 AI adds Middleware")
        .unwrap();

    // C5: AI creates event_bus.py (new file, no conflict)
    let mut bus = repo.filename("event_bus.py");
    bus.set_contents(crate::lines![
        "class EventBus:".ai(),
        "    def __init__(self): self.handlers = {}".ai(),
        "    def on(self, event, fn): self.handlers.setdefault(event, []).append(fn)".ai(),
        "    def emit(self, event, *args):".ai(),
        "        for fn in self.handlers.get(event, []): fn(*args)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 AI adds EventBus")
        .unwrap();

    // Rebase — C2 will conflict on dispatcher.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on dispatcher.py at C2"
    );

    // AI resolves C2: 12-line process() implementation (all .ai() except class line)
    let mut conflict_dispatcher = repo.filename("dispatcher.py");
    conflict_dispatcher.set_contents(crate::lines![
        human_context.expected_line("class Dispatcher:"),
        "    def process(self, msg):".ai(),
        "        # AI merge: validates and parses, as in feature branch".ai(),
        "        msg = msg.strip()".ai(),
        "        if not msg: raise ValueError('empty message')".ai(),
        "        tokens = msg.split()".ai(),
        "        cmd = tokens[0].lower()".ai(),
        "        args = tokens[1:]".ai(),
        "        return {'cmd': cmd, 'args': args, 'raw': msg}".ai(),
        "    def _noop(self, args): return None".ai(),
        "    def __repr__(self): return f'Dispatcher()'".ai(),
        human_context.expected_line("    pass"),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': base_handler.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["base_handler.py"]);

    // C2': dispatcher.py only (AI-resolved: ~10 AI lines)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["dispatcher.py"]);

    // C3': router.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["router.py"]);

    // C4': middleware.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["middleware.py"]);

    // C5': event_bus.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["event_bus.py"]);

    human_context.assert_metadata_humans(&repo, &chain[1], "c2'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_then_more_ai_builds_on_result,
    test_conflict_ai_resolves_then_more_ai_builds_on_result_standard_human,
);
