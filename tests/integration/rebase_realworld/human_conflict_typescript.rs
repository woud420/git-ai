use super::*;

/// Test 3: TypeScript api.ts — feature adds AI REST handlers, main adds an
/// import at the top that conflicts with feature's C3.  C1'–C2' accumulate
/// dto.ts and service.ts; C3' loses api.ts attribution; C4'–C5' add more files.
#[test]
fn test_human_conflict_typescript_api_c3_conflicts_accumulation_intact() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/api.ts",
        "// api module\nexport {};\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: replaces the export line in api.ts — conflicts with feature's C3 which also replaces it
    repo.commit_untracked_file(
        "src/api.ts",
        "// api module\nexport { version };\n",
        "main: export version",
    );
    repo.commit_untracked_file(
        "src/server.ts",
        "import express from 'express';\nconst app = express();\napp.listen(3000);\n",
        "main: add server",
    );
    repo.commit_untracked_file(
        "src/config.ts",
        "export const PORT = parseInt(process.env.PORT ?? '3000', 10);\n",
        "main: add config",
    );
    repo.commit_untracked_file(
        "src/logger.ts",
        "export const log = (msg: string) => console.log(`[LOG] ${msg}`);\n",
        "main: add logger",
    );
    repo.commit_untracked_file(
        "tsconfig.json",
        "{\"compilerOptions\":{\"target\":\"ES2020\",\"module\":\"commonjs\",\"strict\":true}}\n",
        "main: add tsconfig",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates dto.ts
    let mut dto = repo.filename("src/dto.ts");
    dto.set_contents(crate::lines![
        "export interface CreateUserDto {".ai(),
        "  name: string;".ai(),
        "  email: string;".ai(),
        "  password: string;".ai(),
        "}".ai(),
        "".ai(),
        "export interface UpdateUserDto {".ai(),
        "  name?: string;".ai(),
        "  email?: string;".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add user DTOs").unwrap();

    // C2: AI creates service.ts
    let mut service = repo.filename("src/service.ts");
    service.set_contents(crate::lines![
        "import { CreateUserDto, UpdateUserDto } from './dto';".ai(),
        "const users: Map<number, any> = new Map();".ai(),
        "let nextId = 1;".ai(),
        "export const createUser = (dto: CreateUserDto) => { const u = { id: nextId++, ...dto }; users.set(u.id, u); return u; };".ai(),
        "export const getUser = (id: number) => users.get(id);".ai(),
        "export const updateUser = (id: number, dto: UpdateUserDto) => { const u = users.get(id); if (u) Object.assign(u, dto); return u; };".ai(),
        "export const deleteUser = (id: number) => users.delete(id);".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add user service")
        .unwrap();

    // C3: AI edits api.ts to add route handlers — WILL CONFLICT with main's express import
    let mut api = repo.filename("src/api.ts");
    api.replace_at(1, "import { createUser, getUser } from './service';".ai());
    repo.stage_all_and_commit("feat: C3 add route imports to api.ts")
        .unwrap();

    // C4: AI creates middleware.ts
    let mut mw = repo.filename("src/middleware.ts");
    mw.set_contents(crate::lines![
        "import { Request, Response, NextFunction } from 'express';".ai(),
        "".ai(),
        "export const errorHandler = (err: Error, _req: Request, res: Response, _next: NextFunction) => {".ai(),
        "  console.error(err.stack);".ai(),
        "  res.status(500).json({ error: err.message });".ai(),
        "};".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add error middleware")
        .unwrap();

    // C5: AI creates validators.ts
    let mut validators = repo.filename("src/validators.ts");
    validators.set_contents(crate::lines![
        "export const isEmail = (s: string) => /^[^@]+@[^@]+\\.[^@]+$/.test(s);".ai(),
        "export const isNonEmpty = (s: string) => s.trim().length > 0;".ai(),
        "export const isPositiveInt = (n: number) => Number.isInteger(n) && n > 0;".ai(),
        "export const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, n));".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add validators")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/api.ts at C3"
    );

    // Human resolves: keep both the export and the new import
    fs::write(
        repo.path().join("src/api.ts"),
        "// api module\nexport { version };\nimport { createUser, getUser } from './service';\n",
    )
    .unwrap();
    repo.git(&["add", "src/api.ts"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': dto.ts only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/dto.ts"]);

    // C2': service.ts only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/service.ts"]);

    // C3': api.ts human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &[]);

    // C4': middleware.ts only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/middleware.ts"]);

    // C5': validators.ts only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/validators.ts"]);
}

/// Test 6: TypeScript store.ts — the entire feature file is written by the AI
/// via fs::write + git_og add + checkpoint (simulating an AI-created file).
/// Main edits the same file causing conflict on C1; human resolves.
/// No file is attributed in C1' (human resolved the only AI file in that commit).
/// C2'–C5' accumulate actions.ts, selectors.ts, reducers.ts, hooks.ts.
#[test]
fn test_human_conflict_typescript_store_ai_created_file_conflict() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/store.ts",
        "export const store = {};\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: modifies store.ts initial export → conflict with feature C1
    repo.commit_untracked_file(
        "src/store.ts",
        "import { createStore } from 'redux';\nexport const store = createStore(() => ({}));\n",
        "main: convert store to redux",
    );
    repo.commit_untracked_file(
        "src/index.ts",
        "export { store } from './store';\n",
        "main: re-export store",
    );
    repo.commit_untracked_file(
        "src/types.ts",
        "export type RootState = ReturnType<typeof import('./store').store.getState>;\n",
        "main: add RootState type",
    );
    repo.commit_untracked_file(
        "src/constants.ts",
        "export const ACTIONS = { INCREMENT: 'INCREMENT', DECREMENT: 'DECREMENT' } as const;\n",
        "main: add action constants",
    );
    repo.commit_untracked_file(
        "package.json",
        "{\"name\":\"app\",\"version\":\"1.0.0\",\"dependencies\":{\"redux\":\"^4.0.0\"}}\n",
        "main: add package.json",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI writes store.ts from scratch via fs::write + checkpoint
    let store_content = "import { configureStore } from '@reduxjs/toolkit';\nimport { counterSlice } from './reducers';\nexport const store = configureStore({ reducer: { counter: counterSlice.reducer } });\nexport type AppDispatch = typeof store.dispatch;\n";
    fs::write(repo.path().join("src/store.ts"), store_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/store.ts"])
        .unwrap();
    repo.stage_all_and_commit("feat: C1 AI rewrites store with redux toolkit")
        .unwrap();

    // C2: AI creates actions.ts
    let mut actions = repo.filename("src/actions.ts");
    actions.set_contents(crate::lines![
        "export const increment = () => ({ type: 'INCREMENT' as const });".ai(),
        "export const decrement = () => ({ type: 'DECREMENT' as const });".ai(),
        "export const reset = () => ({ type: 'RESET' as const });".ai(),
        "export type Action = ReturnType<typeof increment | typeof decrement | typeof reset>;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add action creators")
        .unwrap();

    // C3: AI creates selectors.ts
    let mut selectors = repo.filename("src/selectors.ts");
    selectors.set_contents(crate::lines![
        "import { RootState } from './types';".ai(),
        "export const selectCount = (state: RootState) => state.counter.value;".ai(),
        "export const selectIsPositive = (state: RootState) => state.counter.value > 0;".ai(),
        "export const selectIsZero = (state: RootState) => state.counter.value === 0;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add selectors").unwrap();

    // C4: AI creates reducers.ts
    let mut reducers = repo.filename("src/reducers.ts");
    reducers.set_contents(crate::lines![
        "import { createSlice } from '@reduxjs/toolkit';".ai(),
        "export const counterSlice = createSlice({".ai(),
        "  name: 'counter',".ai(),
        "  initialState: { value: 0 },".ai(),
        "  reducers: {".ai(),
        "    increment: state => { state.value += 1; },".ai(),
        "    decrement: state => { state.value -= 1; },".ai(),
        "    reset: state => { state.value = 0; },".ai(),
        "  },".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add counter reducer")
        .unwrap();

    // C5: AI creates hooks.ts
    let mut hooks = repo.filename("src/hooks.ts");
    hooks.set_contents(crate::lines![
        "import { TypedUseSelectorHook, useDispatch, useSelector } from 'react-redux';".ai(),
        "import type { AppDispatch } from './store';".ai(),
        "import type { RootState } from './types';".ai(),
        "export const useAppDispatch = () => useDispatch<AppDispatch>();".ai(),
        "export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add typed hooks")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/store.ts at C1"
    );

    // Human resolves by writing a merged store file
    fs::write(
        repo.path().join("src/store.ts"),
        "import { createStore } from 'redux';\nimport { configureStore } from '@reduxjs/toolkit';\nexport const store = configureStore({ reducer: {} });\nexport type AppDispatch = typeof store.dispatch;\n",
    ).unwrap();
    repo.git(&["add", "src/store.ts"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': store.ts human-resolved → AI content survived → store.ts IS in note
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/store.ts"]);

    // C2': actions.ts only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/actions.ts"]);

    // C3': selectors.ts only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/selectors.ts"]);

    // C4': reducers.ts only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/reducers.ts"]);

    // C5': hooks.ts only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/hooks.ts"]);
}

/// Test 9: TypeScript component.tsx — AI writes entire component file (via
/// fs::write + checkpoint), main adds a style import that conflicts on C2.
/// C1' accumulates hooks.ts; C2' loses component.tsx; C3'–C5' add context.ts,
/// provider.tsx, types.ts normally.
#[test]
fn test_human_conflict_typescript_component_ai_created_c2_conflict() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/Component.tsx",
        "export const Component = () => null;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds a CSS import to Component.tsx → conflict with feature C2's rewrite
    repo.commit_untracked_file(
        "src/Component.tsx",
        "import './Component.css';\nexport const Component = () => null;\n",
        "main: add CSS import to Component",
    );
    repo.commit_untracked_file(
        "src/Component.css",
        ".component { display: flex; }\n",
        "main: add component styles",
    );
    repo.commit_untracked_file(
        "src/App.tsx",
        "import { Component } from './Component';\nexport const App = () => <Component />;\n",
        "main: add App",
    );
    repo.commit_untracked_file("src/index.tsx",
        "import React from 'react';\nimport ReactDOM from 'react-dom';\nimport { App } from './App';\nReactDOM.render(<App />, document.getElementById('root'));\n",
        "main: add entry point",
    );
    repo.commit_untracked_file(
        "src/theme.ts",
        "export const theme = { primary: '#007bff', secondary: '#6c757d' };\n",
        "main: add theme",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates hooks.ts
    let mut custom_hooks = repo.filename("src/useCounter.ts");
    custom_hooks.set_contents(crate::lines![
        "import { useState, useCallback } from 'react';".ai(),
        "".ai(),
        "export const useCounter = (initial = 0) => {".ai(),
        "  const [count, setCount] = useState(initial);".ai(),
        "  const increment = useCallback(() => setCount(c => c + 1), []);".ai(),
        "  const decrement = useCallback(() => setCount(c => c - 1), []);".ai(),
        "  const reset = useCallback(() => setCount(initial), [initial]);".ai(),
        "  return { count, increment, decrement, reset };".ai(),
        "};".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add useCounter hook")
        .unwrap();

    // C2: AI rewrites Component.tsx via fs::write + checkpoint — WILL CONFLICT
    let component_content = "import React from 'react';\nimport { useCounter } from './useCounter';\n\nexport const Component: React.FC = () => {\n  const { count, increment, decrement, reset } = useCounter();\n  return <div><button onClick={decrement}>-</button><span>{count}</span><button onClick={increment}>+</button><button onClick={reset}>reset</button></div>;\n};\n";
    fs::write(repo.path().join("src/Component.tsx"), component_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/Component.tsx"])
        .unwrap();
    repo.stage_all_and_commit("feat: C2 AI rewrites Component with useCounter")
        .unwrap();

    // C3: AI creates context.ts
    let mut context = repo.filename("src/context.ts");
    context.set_contents(crate::lines![
        "import React from 'react';".ai(),
        "export interface AppContextValue { theme: string; locale: string; }".ai(),
        "export const AppContext = React.createContext<AppContextValue>({ theme: 'light', locale: 'en' });".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add AppContext")
        .unwrap();

    // C4: AI creates provider.tsx
    let mut provider = repo.filename("src/provider.tsx");
    provider.set_contents(crate::lines![
        "import React, { useState } from 'react';".ai(),
        "import { AppContext } from './context';".ai(),
        "".ai(),
        "export const AppProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {".ai(),
        "  const [theme, setTheme] = useState('light');".ai(),
        "  return <AppContext.Provider value={{ theme, locale: 'en' }}>{children}</AppContext.Provider>;".ai(),
        "};".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add AppProvider")
        .unwrap();

    // C5: AI creates types.ts
    let mut types_file = repo.filename("src/types.ts");
    types_file.set_contents(crate::lines![
        "export type Theme = 'light' | 'dark';".ai(),
        "export type Locale = 'en' | 'fr' | 'de';".ai(),
        "export interface UserPrefs { theme: Theme; locale: Locale; }".ai(),
        "export type Handler<T = void> = (e: React.SyntheticEvent) => T;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add shared types")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/Component.tsx at C2"
    );

    // Human resolves: keep both CSS import and the new component body
    fs::write(
        repo.path().join("src/Component.tsx"),
        "import './Component.css';\nimport React from 'react';\nimport { useCounter } from './useCounter';\n\nexport const Component: React.FC = () => {\n  const { count, increment, decrement } = useCounter();\n  return <div>{count}</div>;\n};\n",
    ).unwrap();
    repo.git(&["add", "src/Component.tsx"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': useCounter.ts only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/useCounter.ts"]);

    // C2': Component.tsx human-resolved → AI content survived → Component.tsx IS in note
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/Component.tsx"]);

    // C3': context.ts only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/context.ts"]);

    // C4': provider.tsx only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/provider.tsx"]);

    // C5': types.ts only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/types.ts"]);
}

crate::reuse_tests_in_worktree!(
    test_human_conflict_typescript_api_c3_conflicts_accumulation_intact,
    test_human_conflict_typescript_store_ai_created_file_conflict,
    test_human_conflict_typescript_component_ai_created_c2_conflict,
);
