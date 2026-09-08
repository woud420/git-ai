use super::{
    ExpectedLineExt, TestRepo, assert_blame_sample_at_commit, assert_note_base_commit_matches,
    assert_note_files_exact, assert_note_no_forbidden_files, fs, get_commit_chain,
};

/// Test 10: Shared file grows AND each commit adds a unique helper file.
/// shared_util.js prepended by main; feature appends 8 lines to it and
/// creates helpers/X.js per commit. Checks cumulative file sets at every SHA.
#[test]
fn test_slow_path_file_grows_then_unique_files_each_commit() {
    let repo = TestRepo::new();

    // Initial: shared_util.js with trailing newline
    repo.commit_untracked_file(
        "shared_util.js",
        "export const VERSION = '1.0';\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend 'use strict' directive (forces slow path)
    repo.commit_untracked_file(
        "shared_util.js",
        "'use strict';\n\nexport const VERSION = '1.0';\n",
        "main: prepend use strict to shared_util.js",
    );
    repo.commit_untracked_file(
        "package.json",
        "{\"name\":\"helpers\",\"version\":\"1.0.0\",\"type\":\"module\"}\n",
        "main: add package.json",
    );
    repo.commit_untracked_file(
        ".eslintrc.json",
        "{\"env\":{\"es2022\":true},\"extends\":[\"eslint:recommended\"]}\n",
        "main: add eslint config",
    );
    repo.commit_untracked_file(
        "vitest.config.js",
        "export default {test:{environment:'node'}};\n",
        "main: add vitest config",
    );
    repo.commit_untracked_file(
        "README.md",
        "# Helpers\n\nA collection of JavaScript helper modules.\n",
        "main: add README",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 8 AI lines to shared_util.js + create helpers/date.js (6 AI lines)
    let mut shared = repo.filename("shared_util.js");
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export function clamp(n, min, max) { return Math.min(Math.max(n, min), max); }".ai(),
        "export function lerp(a, b, t) { return a + (b - a) * t; }".ai(),
        "export function noop() {}".ai(),
        "export const identity = x => x;".ai(),
        "export function once(fn) { let called = false, result; return (...a) => { if (!called) { called = true; result = fn(...a); } return result; }; }".ai(),
        "export function memoize(fn) { const cache = new Map(); return (...a) => { const k = JSON.stringify(a); if (!cache.has(k)) cache.set(k, fn(...a)); return cache.get(k); }; }".ai(),
        "export function pipe(...fns) { return x => fns.reduce((v, f) => f(v), x); }".ai(),
        "export function compose(...fns) { return x => fns.reduceRight((v, f) => f(v), x); }".ai(),
    ]);
    // Ensure the helpers directory exists
    let helpers_dir = repo.path().join("helpers");
    fs::create_dir_all(&helpers_dir).expect("create helpers dir");
    let mut date_helper = repo.filename("helpers/date.js");
    date_helper.set_contents(crate::lines![
        "export const now = () => new Date();".ai(),
        "export const today = () => { const d = new Date(); d.setHours(0,0,0,0); return d; };".ai(),
        "export const addDays = (d, n) => { const r = new Date(d); r.setDate(r.getDate()+n); return r; };".ai(),
        "export const formatISO = d => d.toISOString().slice(0, 10);".ai(),
        "export const isWeekend = d => d.getDay() === 0 || d.getDay() === 6;".ai(),
        "export const diffMs = (a, b) => Math.abs(new Date(a) - new Date(b));".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 grow shared_util.js + helpers/date.js")
        .unwrap();

    // C2: append 8 more AI lines to shared_util.js + create helpers/string.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const identity = x => x;".ai(),
        "export const once = fn => { let c=false,r; return (...a) => { if(!c){c=true;r=fn(...a);} return r; }; };".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); if(!m.has(k)) m.set(k,fn(...a)); return m.get(k); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "".ai(),
        "export function curry(fn) { return function curried(...args) { return args.length >= fn.length ? fn(...args) : (...more) => curried(...args, ...more); }; }".ai(),
        "export function partial(fn, ...preset) { return (...args) => fn(...preset, ...args); }".ai(),
        "export function flip(fn) { return (a, b, ...rest) => fn(b, a, ...rest); }".ai(),
        "export function tap(fn) { return x => { fn(x); return x; }; }".ai(),
        "export const constant = v => () => v;".ai(),
        "export const always = constant;".ai(),
        "export const negate = pred => (...args) => !pred(...args);".ai(),
    ]);
    let mut string_helper = repo.filename("helpers/string.js");
    string_helper.set_contents(crate::lines![
        "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);".ai(),
        "export const kebabToCamel = s => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());".ai(),
        "export const camelToKebab = s => s.replace(/[A-Z]/g, m => `-${m.toLowerCase()}`);".ai(),
        "export const truncate = (s, n) => s.length <= n ? s : s.slice(0, n-3) + '...';".ai(),
        "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');".ai(),
        "export const words = s => s.trim().split(/\\s+/).filter(Boolean);".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 grow shared_util.js + helpers/string.js")
        .unwrap();

    // C3: append 8 more AI lines to shared_util.js + create helpers/array.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const identity = x => x;".ai(),
        "export const once = fn => { let c=false,r; return (...a) => { if(!c){c=true;r=fn(...a);} return r; }; };".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); return m.has(k)?m.get(k):(m.set(k,fn(...a)),m.get(k)); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "export const curry = fn => function c(...a) { return a.length>=fn.length ? fn(...a) : (...b)=>c(...a,...b); };".ai(),
        "export const partial = (fn,...p) => (...a) => fn(...p,...a);".ai(),
        "export const negate = pred => (...a) => !pred(...a);".ai(),
        "".ai(),
        "export function debounce(fn, ms) { let t; return (...a) => { clearTimeout(t); t = setTimeout(()=>fn(...a), ms); }; }".ai(),
        "export function throttle(fn, ms) { let ok=true; return (...a) => { if(ok) { ok=false; fn(...a); setTimeout(()=>ok=true,ms); } }; }".ai(),
        "export function trampoline(fn) { return (...a) => { let r=fn(...a); while(typeof r==='function') r=r(); return r; }; }".ai(),
        "export function juxt(...fns) { return (...a) => fns.map(f=>f(...a)); }".ai(),
        "export const when = (pred, fn) => (...a) => pred(...a) ? fn(...a) : a[0];".ai(),
    ]);
    let mut array_helper = repo.filename("helpers/array.js");
    array_helper.set_contents(crate::lines![
        "export const unique = arr => [...new Set(arr)];".ai(),
        "export const flatten = arr => arr.flat(Infinity);".ai(),
        "export const chunk = (arr, n) => Array.from({length: Math.ceil(arr.length/n)}, (_,i) => arr.slice(i*n, i*n+n));".ai(),
        "export const groupBy = (arr, key) => arr.reduce((g, item) => ((g[item[key]] ??= []).push(item), g), {});".ai(),
        "export const zip = (...arrays) => arrays[0].map((_,i) => arrays.map(a=>a[i]));".ai(),
        "export const intersection = (a, b) => a.filter(x => b.includes(x));".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 grow shared_util.js + helpers/array.js")
        .unwrap();

    // C4: append 8 more AI lines to shared_util.js + create helpers/object.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const identity = x => x;".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); return m.has(k)?m.get(k):(m.set(k,fn(...a)),m.get(k)); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "export const curry = fn => function c(...a) { return a.length>=fn.length ? fn(...a) : (...b)=>c(...a,...b); };".ai(),
        "export const debounce = (fn, ms) => { let t; return (...a)=>{ clearTimeout(t); t=setTimeout(()=>fn(...a),ms); }; };".ai(),
        "export const throttle = (fn, ms) => { let ok=true; return (...a)=>{ if(ok){ok=false;fn(...a);setTimeout(()=>ok=true,ms);} }; };".ai(),
        "export const when = (pred, fn) => (...a) => pred(...a) ? fn(...a) : a[0];".ai(),
        "".ai(),
        "export class EventEmitter { #events={}; on(e,f){(this.#events[e]??=[]).push(f);return this;} emit(e,...a){(this.#events[e]??[]).forEach(f=>f(...a));} }".ai(),
        "export const sleep = ms => new Promise(r => setTimeout(r, ms));".ai(),
        "export async function retry(fn, n=3) { for(let i=0;i<n;i++) { try{return await fn();}catch(e){if(i===n-1)throw e;await sleep(100*(i+1));} } }".ai(),
        "export const withTimeout = (p, ms) => Promise.race([p, new Promise((_,r)=>setTimeout(()=>r(new Error('timeout')),ms))]);".ai(),
        "export const deferred = () => { let res,rej; const p=new Promise((r,j)=>{res=r;rej=j;}); return {promise:p,resolve:res,reject:rej}; };".ai(),
    ]);
    let mut object_helper = repo.filename("helpers/object.js");
    object_helper.set_contents(crate::lines![
        "export const pick = (obj, keys) => Object.fromEntries(keys.map(k=>[k,obj[k]]));".ai(),
        "export const omit = (obj, keys) => Object.fromEntries(Object.entries(obj).filter(([k])=>!keys.includes(k)));".ai(),
        "export const deepClone = obj => JSON.parse(JSON.stringify(obj));".ai(),
        "export const isEmpty = obj => Object.keys(obj).length === 0;".ai(),
        "export const mapValues = (obj, fn) => Object.fromEntries(Object.entries(obj).map(([k,v])=>[k,fn(v,k)]));".ai(),
        "export const fromEntries = Object.fromEntries;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 grow shared_util.js + helpers/object.js")
        .unwrap();

    // C5: append 8 more AI lines to shared_util.js + create helpers/number.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const identity = x => x;".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); return m.has(k)?m.get(k):(m.set(k,fn(...a)),m.get(k)); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "export const debounce = (fn, ms) => { let t; return (...a)=>{ clearTimeout(t); t=setTimeout(()=>fn(...a),ms); }; };".ai(),
        "export const sleep = ms => new Promise(r => setTimeout(r, ms));".ai(),
        "export async function retry(fn, n=3) { for(let i=0;i<n;i++) { try{return await fn();}catch(e){if(i===n-1)throw e;await sleep(100*(i+1));} } }".ai(),
        "export const deferred = () => { let res,rej; const p=new Promise((r,j)=>{res=r;rej=j;}); return {promise:p,resolve:res,reject:rej}; };".ai(),
        "".ai(),
        "export function deepEqual(a, b) {".ai(),
        "    if (a === b) return true;".ai(),
        "    if (typeof a !== typeof b) return false;".ai(),
        "    if (Array.isArray(a)) return a.length===b.length && a.every((v,i)=>deepEqual(v,b[i]));".ai(),
        "    if (typeof a === 'object' && a && b) {".ai(),
        "        const ka=Object.keys(a), kb=Object.keys(b);".ai(),
        "        return ka.length===kb.length && ka.every(k=>deepEqual(a[k],b[k]));".ai(),
        "    }".ai(),
        "    return false;".ai(),
        "}".ai(),
    ]);
    let mut number_helper = repo.filename("helpers/number.js");
    number_helper.set_contents(crate::lines![
        "export const round = (n, d=0) => Math.round(n * 10**d) / 10**d;".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const isPrime = n => n>1 && Array.from({length:Math.sqrt(n)|0},(_, i)=>i+2).every(i=>n%i!==0);".ai(),
        "export const gcd = (a, b) => b === 0 ? a : gcd(b, a % b);".ai(),
        "export const formatBytes = n => { const u=['B','KB','MB','GB']; let i=0; while(n>=1024&&i<3){n/=1024;i++;} return `${n.toFixed(1)}${u[i]}`; };".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 grow shared_util.js + helpers/number.js")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {shared_util.js, helpers/date.js}; no future helpers
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(
        &repo,
        &chain[0],
        "sha0_files",
        &["shared_util.js", "helpers/date.js"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "helpers/string.js",
            "helpers/array.js",
            "helpers/object.js",
            "helpers/number.js",
        ],
    );

    // sha1 = C2': {shared_util.js, helpers/string.js}; no future helpers
    // C2 added curry/partial/flip/tap/negate to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["shared_util.js", "helpers/string.js"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["helpers/array.js", "helpers/object.js", "helpers/number.js"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "shared_util.js",
        "sha1_shared_curry",
        &[
            ("export function curry", true),
            ("export function partial", true),
            ("export const negate", true),
        ],
    );
    // helpers/date.js (from C1) is a prior file at chain[1] — fast path, verify attribution intact
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "helpers/date.js",
        "chain1_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );

    // sha2 = C3': {shared_util.js, helpers/array.js}; no object or number yet
    // C3 added debounce/throttle/trampoline/juxt/when to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["shared_util.js", "helpers/array.js"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["helpers/object.js", "helpers/number.js"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "shared_util.js",
        "sha2_shared_debounce",
        &[
            ("export function debounce", true),
            ("export function throttle", true),
            ("export function trampoline", true),
        ],
    );
    // helpers/date.js (from C1) and helpers/string.js (from C2) are prior files at chain[2]
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "helpers/date.js",
        "chain2_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "helpers/string.js",
        "chain2_prior_string_js",
        &[
            (
                "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);",
                true,
            ),
            (
                "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');",
                true,
            ),
        ],
    );

    // sha3 = C4': {shared_util.js, helpers/object.js}; no number yet
    // C4 added EventEmitter/sleep/retry/withTimeout/deferred to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(
        &repo,
        &chain[3],
        "sha3_files",
        &["shared_util.js", "helpers/object.js"],
    );
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["helpers/number.js"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "shared_util.js",
        "sha3_shared_eventemitter",
        &[
            ("export class EventEmitter", true),
            ("export const sleep", true),
            ("export async function retry", true),
        ],
    );
    // helpers/date.js (C1), helpers/string.js (C2), and helpers/array.js (C3) are prior files at chain[3]
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "helpers/date.js",
        "chain3_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "helpers/string.js",
        "chain3_prior_string_js",
        &[
            (
                "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);",
                true,
            ),
            (
                "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "helpers/array.js",
        "chain3_prior_array_js",
        &[
            ("export const unique = arr => [...new Set(arr)];", true),
            (
                "export const chunk = (arr, n) => Array.from({length: Math.ceil(arr.length/n)}, (_,i) => arr.slice(i*n, i*n+n));",
                true,
            ),
        ],
    );

    // sha4 = C5': {shared_util.js, helpers/number.js}
    // C5 added deepEqual to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["shared_util.js", "helpers/number.js"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "shared_util.js",
        "sha4_shared_deepequal",
        &[
            ("export function deepEqual", true),
            ("if (Array.isArray(a))", true),
            ("return false;", true),
        ],
    );
    // helpers/date.js (C1), string.js (C2), array.js (C3), and object.js (C4) are prior files at chain[4]
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/date.js",
        "chain4_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/string.js",
        "chain4_prior_string_js",
        &[
            (
                "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);",
                true,
            ),
            (
                "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/array.js",
        "chain4_prior_array_js",
        &[
            ("export const unique = arr => [...new Set(arr)];", true),
            (
                "export const chunk = (arr, n) => Array.from({length: Math.ceil(arr.length/n)}, (_,i) => arr.slice(i*n, i*n+n));",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/object.js",
        "chain4_prior_object_js",
        &[
            (
                "export const pick = (obj, keys) => Object.fromEntries(keys.map(k=>[k,obj[k]]));",
                true,
            ),
            (
                "export const deepClone = obj => JSON.parse(JSON.stringify(obj));",
                true,
            ),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_file_grows_then_unique_files_each_commit,);
