use super::*;

/// Test 3: TypeScript routes.ts — upstream prepends a comment, feature appends
/// endpoint handler functions. Blame at sha0 checks human lines at top.
#[test]
fn test_slow_path_typescript_routes_main_prepends_feature_adds_handlers() {
    let repo = TestRepo::new();

    // Initial: src/routes.ts with trailing newline
    repo.commit_untracked_file(
        "src/routes.ts",
        "import express from 'express';\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend auto-generated comment (forces slow path)
    repo.commit_untracked_file(
        "src/routes.ts",
        "// Auto-generated routes\nimport express from 'express';\n",
        "main: prepend auto-generated comment",
    );
    repo.commit_untracked_file("src/middleware.ts",
        "export const logger = (req: any, res: any, next: any) => { console.log(req.method, req.path); next(); };\n",
        "main: add logger middleware",
    );
    repo.commit_untracked_file("src/types.ts",
        "export interface User { id: number; email: string; name: string; }\nexport interface ApiResponse<T> { data: T; status: number; }\n",
        "main: add shared types",
    );
    repo.commit_untracked_file("tsconfig.json",
        "{\"compilerOptions\":{\"target\":\"ES2020\",\"module\":\"commonjs\",\"strict\":true,\"outDir\":\"dist\"},\"include\":[\"src\"]}\n",
        "main: add tsconfig",
    );
    repo.commit_untracked_file("package.json",
        "{\"name\":\"api\",\"version\":\"1.0.0\",\"scripts\":{\"build\":\"tsc\",\"start\":\"node dist/index.js\"}}\n",
        "main: add package.json",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append /users GET handler (8 AI lines)
    let mut routes = repo.filename("src/routes.ts");
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => {".ai(),
        "  try {".ai(),
        "    const users = await UserService.findAll();".ai(),
        "    res.json({ data: users, status: 200 });".ai(),
        "  } catch (err) {".ai(),
        "    res.status(500).json({ error: String(err) });".ai(),
        "  }".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add GET /users route")
        .unwrap();

    // C2: append /users POST handler (8 AI lines)
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => {".ai(),
        "  try {".ai(),
        "    const users = await UserService.findAll();".ai(),
        "    res.json({ data: users, status: 200 });".ai(),
        "  } catch (err) {".ai(),
        "    res.status(500).json({ error: String(err) });".ai(),
        "  }".ai(),
        "});".ai(),
        "".ai(),
        "router.post('/users', async (req, res) => {".ai(),
        "  const { email, name } = req.body;".ai(),
        "  if (!email || !name) return res.status(400).json({ error: 'email and name required' });"
            .ai(),
        "  const user = await UserService.create({ email, name });".ai(),
        "  res.status(201).json({ data: user, status: 201 });".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add POST /users route")
        .unwrap();

    // C3: append /users/:id GET handler (8 AI lines)
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => {".ai(),
        "  const users = await UserService.findAll();".ai(),
        "  res.json({ data: users, status: 200 });".ai(),
        "});".ai(),
        "".ai(),
        "router.post('/users', async (req, res) => {".ai(),
        "  const { email, name } = req.body;".ai(),
        "  const user = await UserService.create({ email, name });".ai(),
        "  res.status(201).json({ data: user, status: 201 });".ai(),
        "});".ai(),
        "".ai(),
        "router.get('/users/:id', async (req, res) => {".ai(),
        "  const id = parseInt(req.params.id, 10);".ai(),
        "  const user = await UserService.findById(id);".ai(),
        "  if (!user) return res.status(404).json({ error: 'Not found' });".ai(),
        "  res.json({ data: user, status: 200 });".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add GET /users/:id route")
        .unwrap();

    // C4: append /users/:id PUT handler (8 AI lines)
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => { const users = await UserService.findAll(); res.json({ data: users }); });".ai(),
        "router.post('/users', async (req, res) => { const user = await UserService.create(req.body); res.status(201).json({ data: user }); });".ai(),
        "router.get('/users/:id', async (req, res) => { const user = await UserService.findById(+req.params.id); res.json({ data: user }); });".ai(),
        "".ai(),
        "router.put('/users/:id', async (req, res) => {".ai(),
        "  const id = parseInt(req.params.id, 10);".ai(),
        "  const updates = req.body;".ai(),
        "  const user = await UserService.update(id, updates);".ai(),
        "  if (!user) return res.status(404).json({ error: 'Not found' });".ai(),
        "  res.json({ data: user, status: 200 });".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add PUT /users/:id route")
        .unwrap();

    // C5: append /users/:id DELETE handler (8 AI lines) + export
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => { const users = await UserService.findAll(); res.json({ data: users }); });".ai(),
        "router.post('/users', async (req, res) => { const user = await UserService.create(req.body); res.status(201).json({ data: user }); });".ai(),
        "router.get('/users/:id', async (req, res) => { const user = await UserService.findById(+req.params.id); res.json({ data: user }); });".ai(),
        "router.put('/users/:id', async (req, res) => { const user = await UserService.update(+req.params.id, req.body); res.json({ data: user }); });".ai(),
        "".ai(),
        "router.delete('/users/:id', async (req, res) => {".ai(),
        "  const id = parseInt(req.params.id, 10);".ai(),
        "  const deleted = await UserService.delete(id);".ai(),
        "  if (!deleted) return res.status(404).json({ error: 'Not found' });".ai(),
        "  res.status(204).send();".ai(),
        "});".ai(),
        "".ai(),
        "export default router;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add DELETE /users/:id + export")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': src/routes.ts with ~8 AI lines
    // blame: line 1 = human (// Auto-generated routes), line 2 = human (import express),
    // then AI lines start
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[0],
        "src/routes.ts",
        "sha0_blame",
        &[
            ("// Auto-generated routes", false),
            ("import express from 'express';", false),
            ("const router = express.Router();", true),
            ("router.get('/users'", true),
            ("try {", true),
            ("const users = await UserService.findAll()", true),
        ],
    );

    // sha1 = C2': only C2's delta
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/routes.ts",
        "sha1_blame_new",
        &[
            ("router.post('/users'", true),
            ("email and name required", true),
        ],
    );

    // sha2 = C3': only C3's delta
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/routes.ts",
        "sha2_blame_new",
        &[
            ("router.get('/users/:id'", true),
            ("UserService.findById", true),
        ],
    );

    // sha3 = C4': only C4's delta
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/routes.ts",
        "sha3_blame_new",
        &[
            ("router.put('/users/:id'", true),
            ("UserService.update", true),
        ],
    );

    // sha4 = C5': only C5's delta
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/routes.ts",
        "sha4_blame_new",
        &[
            ("router.delete('/users/:id'", true),
            ("export default router", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(
    test_slow_path_typescript_routes_main_prepends_feature_adds_handlers,
);
