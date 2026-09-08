use super::{
    ExpectedLineExt, TestRepo, assert_blame_at_commit, assert_blame_sample_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, assert_note_no_forbidden_files,
    get_commit_chain,
};

fn assert_prior_utilities(
    repo: &TestRepo,
    sha: &str,
    chain_index: usize,
    file_range: std::ops::Range<usize>,
) {
    let prior_samples = [
        (
            "date_utils.js",
            ["export function formatDate", "export function addDays"],
        ),
        (
            "string_utils.js",
            ["export const capitalize", "export const slugify"],
        ),
        (
            "array_utils.js",
            ["export const unique", "export const flatten"],
        ),
        (
            "object_utils.js",
            ["export const pick", "export const deepClone"],
        ),
        (
            "number_utils.js",
            ["export const clamp", "export const lerp"],
        ),
        (
            "dom_utils.js",
            ["export const $ = sel", "export const $$ = sel"],
        ),
        (
            "fetch_utils.js",
            [
                "export async function getJSON",
                "export async function postJSON",
            ],
        ),
        (
            "storage_utils.js",
            ["export const ls = {", "export const ss = {"],
        ),
        (
            "event_utils.js",
            ["export function debounce", "export function throttle"],
        ),
    ];
    for (file, samples) in &prior_samples[file_range] {
        assert_blame_sample_at_commit(
            repo,
            sha,
            file,
            &format!("chain{chain_index}_prior_{file}"),
            &samples.map(|line| (line, true)),
        );
    }
}

#[test]
fn test_fast_path_10_commits_javascript_utilities() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("index.js");
    init.set_contents(crate::lines!["// JavaScript utility library"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 10 commits, each adding a JS utility file ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: date_utils.js
    let mut fu1 = repo.filename("date_utils.js");
    fu1.set_contents(crate::lines![
        "export function formatDate(date, fmt = 'YYYY-MM-DD') {".ai(),
        "  const d = date instanceof Date ? date : new Date(date);".ai(),
        "  return fmt.replace('YYYY', d.getFullYear()).replace('MM', String(d.getMonth()+1).padStart(2,'0')).replace('DD', String(d.getDate()).padStart(2,'0'));".ai(),
        "}".ai(),
        "export function addDays(date, n) { const d = new Date(date); d.setDate(d.getDate() + n); return d; }".ai(),
        "export function diffDays(a, b) { return Math.floor((new Date(b) - new Date(a)) / 86400000); }".ai(),
        "export function isWeekend(date) { const day = new Date(date).getDay(); return day === 0 || day === 6; }".ai(),
        "export function startOfWeek(date) { const d = new Date(date); d.setDate(d.getDate() - d.getDay()); return d; }".ai(),
    ]);
    repo.stage_all_and_commit("feat: add date utilities")
        .unwrap();

    // C2: string_utils.js
    let mut fu2 = repo.filename("string_utils.js");
    fu2.set_contents(crate::lines![
        "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);".ai(),
        "export const camelToKebab = s => s.replace(/[A-Z]/g, m => `-${m.toLowerCase()}`);".ai(),
        "export const kebabToCamel = s => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());".ai(),
        "export const truncate = (s, n, ellipsis = '...') => s.length <= n ? s : s.slice(0, n - ellipsis.length) + ellipsis;".ai(),
        "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');".ai(),
        "export const countWords = s => s.trim().split(/\\s+/).filter(Boolean).length;".ai(),
        "export const repeat = (s, n) => Array(n).fill(s).join('');".ai(),
        "export const escapeHtml = s => s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');".ai(),
    ]);
    repo.stage_all_and_commit("feat: add string utilities")
        .unwrap();

    // C3: array_utils.js
    let mut fu3 = repo.filename("array_utils.js");
    fu3.set_contents(crate::lines![
        "export const unique = arr => [...new Set(arr)];".ai(),
        "export const flatten = arr => arr.reduce((a, b) => a.concat(Array.isArray(b) ? flatten(b) : b), []);".ai(),
        "export const chunk = (arr, size) => Array.from({length: Math.ceil(arr.length/size)}, (_, i) => arr.slice(i*size, i*size+size));".ai(),
        "export const groupBy = (arr, key) => arr.reduce((g, item) => { (g[item[key]] = g[item[key]] || []).push(item); return g; }, {});".ai(),
        "export const sortBy = (arr, key, dir = 'asc') => [...arr].sort((a,b) => dir==='asc' ? (a[key]>b[key]?1:-1) : (a[key]<b[key]?1:-1));".ai(),
        "export const intersection = (a, b) => a.filter(x => b.includes(x));".ai(),
        "export const difference = (a, b) => a.filter(x => !b.includes(x));".ai(),
        "export const zip = (...arrays) => arrays[0].map((_,i) => arrays.map(a => a[i]));".ai(),
    ]);
    repo.stage_all_and_commit("feat: add array utilities")
        .unwrap();

    // C4: object_utils.js
    let mut fu4 = repo.filename("object_utils.js");
    fu4.set_contents(crate::lines![
        "export const pick = (obj, keys) => Object.fromEntries(keys.map(k => [k, obj[k]]));".ai(),
        "export const omit = (obj, keys) => Object.fromEntries(Object.entries(obj).filter(([k]) => !keys.includes(k)));".ai(),
        "export const deepClone = obj => JSON.parse(JSON.stringify(obj));".ai(),
        "export const deepMerge = (a, b) => { const r = {...a}; for (const k in b) r[k] = (typeof b[k]==='object'&&b[k]&&!Array.isArray(b[k])) ? deepMerge(a[k]||{},b[k]) : b[k]; return r; };".ai(),
        "export const flatten_obj = (obj, prefix='') => Object.entries(obj).reduce((a,[k,v]) => typeof v==='object'&&v ? {...a,...flatten_obj(v,prefix+k+'.')} : {...a,[prefix+k]:v}, {});".ai(),
        "export const isEmpty = obj => Object.keys(obj).length === 0;".ai(),
        "export const mapValues = (obj, fn) => Object.fromEntries(Object.entries(obj).map(([k,v]) => [k, fn(v, k)]));".ai(),
        "export const filterKeys = (obj, pred) => Object.fromEntries(Object.entries(obj).filter(([k]) => pred(k)));".ai(),
    ]);
    repo.stage_all_and_commit("feat: add object utilities")
        .unwrap();

    // C5: number_utils.js
    let mut fu5 = repo.filename("number_utils.js");
    fu5.set_contents(crate::lines![
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const round = (n, decimals) => Math.round(n * 10**decimals) / 10**decimals;".ai(),
        "export const formatBytes = n => { const units=['B','KB','MB','GB']; let i=0; while(n>=1024&&i<3){n/=1024;i++;} return `${n.toFixed(1)} ${units[i]}`; };".ai(),
        "export const isPrime = n => { if(n<2) return false; for(let i=2;i<=Math.sqrt(n);i++) if(n%i===0) return false; return true; };".ai(),
        "export const fibonacci = n => n<=1 ? n : fibonacci(n-1)+fibonacci(n-2);".ai(),
        "export const gcd = (a,b) => b===0 ? a : gcd(b, a%b);".ai(),
        "export const range = (start, end, step=1) => Array.from({length:Math.ceil((end-start)/step)},(_,i)=>start+i*step);".ai(),
    ]);
    repo.stage_all_and_commit("feat: add number utilities")
        .unwrap();

    // C6: dom_utils.js
    let mut fu6 = repo.filename("dom_utils.js");
    fu6.set_contents(crate::lines![
        "export const $ = sel => document.querySelector(sel);".ai(),
        "export const $$ = sel => [...document.querySelectorAll(sel)];".ai(),
        "export const on = (el, ev, fn, opts) => { el.addEventListener(ev, fn, opts); return () => el.removeEventListener(ev, fn); };".ai(),
        "export const once = (el, ev, fn) => el.addEventListener(ev, fn, {once: true});".ai(),
        "export const delegate = (root, sel, ev, fn) => on(root, ev, e => { const t = e.target.closest(sel); if(t && root.contains(t)) fn.call(t, e); });".ai(),
        "export const ready = fn => document.readyState !== 'loading' ? fn() : document.addEventListener('DOMContentLoaded', fn);".ai(),
        "export const setStyles = (el, styles) => Object.assign(el.style, styles);".ai(),
        "export const toggleClass = (el, cls, force) => el.classList.toggle(cls, force);".ai(),
    ]);
    repo.stage_all_and_commit("feat: add dom utilities")
        .unwrap();

    // C7: fetch_utils.js
    let mut fu7 = repo.filename("fetch_utils.js");
    fu7.set_contents(crate::lines![
        "export async function getJSON(url, opts={}) { const r = await fetch(url, opts); if(!r.ok) throw new Error(r.statusText); return r.json(); }".ai(),
        "export async function postJSON(url, body, opts={}) { return getJSON(url, {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(body), ...opts}); }".ai(),
        "export async function retry(fn, attempts=3, delay=300) { for(let i=0;i<attempts;i++) { try { return await fn(); } catch(e) { if(i===attempts-1) throw e; await new Promise(r=>setTimeout(r,delay*(i+1))); } } }".ai(),
        "export const withTimeout = (promise, ms) => Promise.race([promise, new Promise((_,r)=>setTimeout(()=>r(new Error('timeout')),ms))]);".ai(),
        "export const buildURL = (base, params) => { const u = new URL(base); Object.entries(params).forEach(([k,v])=>u.searchParams.set(k,v)); return u.toString(); };".ai(),
        "export const isAbsolute = url => /^https?:\\/\\//.test(url);".ai(),
        "export async function downloadBlob(url, filename) { const r = await fetch(url); const b = await r.blob(); const a = document.createElement('a'); a.href = URL.createObjectURL(b); a.download = filename; a.click(); }".ai(),
        "export const memoFetch = (() => { const cache = {}; return async (url) => cache[url] ?? (cache[url] = await getJSON(url)); })();".ai(),
    ]);
    repo.stage_all_and_commit("feat: add fetch utilities")
        .unwrap();

    // C8: storage_utils.js
    let mut fu8 = repo.filename("storage_utils.js");
    fu8.set_contents(crate::lines![
        "export const ls = { get: k => { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } }, set: (k,v) => localStorage.setItem(k, JSON.stringify(v)), del: k => localStorage.removeItem(k), clear: () => localStorage.clear() };".ai(),
        "export const ss = { get: k => { try { return JSON.parse(sessionStorage.getItem(k)); } catch { return null; } }, set: (k,v) => sessionStorage.setItem(k, JSON.stringify(v)), del: k => sessionStorage.removeItem(k) };".ai(),
        "export function createStore(key, initial) { let val = ls.get(key) ?? initial; return { get: () => val, set: v => { val = v; ls.set(key, v); }, reset: () => { val = initial; ls.del(key); } }; }".ai(),
        "export const cookie = { get: name => Object.fromEntries(document.cookie.split(';').map(c=>c.trim().split('=')))[name], set: (name,value,days=7) => { document.cookie = `${name}=${value};max-age=${days*86400};path=/`; }, del: name => cookie.set(name, '', -1) };".ai(),
        "export function withExpiry(key, value, ttl) { ls.set(key, {value, expires: Date.now()+ttl}); }".ai(),
        "export function getWithExpiry(key) { const item = ls.get(key); if(!item) return null; if(Date.now() > item.expires) { ls.del(key); return null; } return item.value; }".ai(),
        "export const hasStorage = (() => { try { localStorage.setItem('_t','1'); localStorage.removeItem('_t'); return true; } catch { return false; } })();".ai(),
        "export function broadcastStore(key, val) { ls.set(key, val); window.dispatchEvent(new StorageEvent('storage', {key, newValue: JSON.stringify(val)})); }".ai(),
    ]);
    repo.stage_all_and_commit("feat: add storage utilities")
        .unwrap();

    // C9: event_utils.js
    let mut fu9 = repo.filename("event_utils.js");
    fu9.set_contents(crate::lines![
        "export class EventEmitter { constructor() { this._events = {}; } on(ev, fn) { (this._events[ev] = this._events[ev]||[]).push(fn); return this; } off(ev, fn) { this._events[ev] = (this._events[ev]||[]).filter(f=>f!==fn); return this; } emit(ev, ...args) { (this._events[ev]||[]).forEach(f=>f(...args)); return this; } once(ev, fn) { const w = (...a) => { fn(...a); this.off(ev, w); }; return this.on(ev, w); } }".ai(),
        "export function debounce(fn, wait) { let t; return function(...a) { clearTimeout(t); t = setTimeout(()=>fn.apply(this,a), wait); }; }".ai(),
        "export function throttle(fn, wait) { let last=0; return function(...a) { const now=Date.now(); if(now-last>=wait) { last=now; return fn.apply(this,a); } }; }".ai(),
        "export function createPubSub() { const subs = {}; return { sub: (t, fn) => (subs[t] = subs[t]||new Set()).add(fn), unsub: (t, fn) => subs[t]?.delete(fn), pub: (t, d) => subs[t]?.forEach(fn => fn(d)) }; }".ai(),
        "export const keyCombo = (keys, fn) => document.addEventListener('keydown', e => { if(keys.every(k => k==='ctrl'?e.ctrlKey:k==='shift'?e.shiftKey:k==='alt'?e.altKey:e.key===k)) fn(e); });".ai(),
        "export function onIdle(fn) { return 'requestIdleCallback' in window ? requestIdleCallback(fn) : setTimeout(fn, 1); }".ai(),
        "export const dispatchCustom = (el, name, detail) => el.dispatchEvent(new CustomEvent(name, {bubbles: true, detail}));".ai(),
        "export function onVisible(el, fn) { const obs = new IntersectionObserver(([e])=>{ if(e.isIntersecting){fn();obs.disconnect();} }); obs.observe(el); return ()=>obs.disconnect(); }".ai(),
    ]);
    repo.stage_all_and_commit("feat: add event utilities")
        .unwrap();

    // C10: validation_utils.js
    let mut fu10 = repo.filename("validation_utils.js");
    fu10.set_contents(crate::lines![
        "export const isEmail = s => /^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/.test(s);".ai(),
        "export const isURL = s => { try { new URL(s); return true; } catch { return false; } };".ai(),
        "export const isPhone = s => /^\\+?[\\d\\s\\-().]{7,20}$/.test(s);".ai(),
        "export const isUUID = s => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s);".ai(),
        "export const minLength = (n, msg) => v => v.length >= n ? null : msg ?? `Min ${n} chars`;".ai(),
        "export const maxLength = (n, msg) => v => v.length <= n ? null : msg ?? `Max ${n} chars`;".ai(),
        "export const required = msg => v => v!=null && v!=='' ? null : msg ?? 'Required';".ai(),
        "export function validate(value, rules) { for(const r of rules) { const err = r(value); if(err) return err; } return null; }".ai(),
    ]);
    repo.stage_all_and_commit("feat: add validation utilities")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "package.json",
        "{\"name\":\"utils\",\"version\":\"1.0.0\",\"type\":\"module\"}\n",
        "build: add package.json",
    );
    repo.commit_untracked_file(".eslintrc.cjs",
        "module.exports={env:{browser:true,es2021:true},extends:['eslint:recommended'],parserOptions:{ecmaVersion:'latest',sourceType:'module'}};\n",
        "lint: add eslint config",
    );
    repo.commit_untracked_file("vitest.config.js",
        "import { defineConfig } from 'vitest/config';\nexport default defineConfig({test:{environment:'jsdom'}});\n",
        "test: add vitest config",
    );
    repo.commit_untracked_file(
        ".prettierrc",
        "{\"singleQuote\":true,\"semi\":false,\"trailingComma\":\"es5\"}\n",
        "style: add prettier config",
    );
    repo.commit_untracked_file(
        "README.md",
        "# JS Utilities\n\nA collection of JavaScript utility functions.\n",
        "docs: add README",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT ALL 10 COMMITS ===
    let chain = get_commit_chain(&repo, 10);

    // sha0 = C1': only date_utils.js
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["date_utils.js"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "string_utils.js",
            "array_utils.js",
            "object_utils.js",
            "number_utils.js",
            "dom_utils.js",
            "fetch_utils.js",
            "storage_utils.js",
            "event_utils.js",
            "validation_utils.js",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "date_utils.js",
        "sha0_blame",
        &[
            ("export function formatDate", true),
            ("const d = date instanceof Date", true),
            ("return fmt.replace", true),
            ("}", true),
            ("export function addDays", true),
            ("export function diffDays", true),
            ("export function isWeekend", true),
            ("export function startOfWeek", true),
        ],
    );

    // sha1 = C2': string_utils.js
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["string_utils.js"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &[
            "array_utils.js",
            "object_utils.js",
            "number_utils.js",
            "dom_utils.js",
            "fetch_utils.js",
            "storage_utils.js",
            "event_utils.js",
            "validation_utils.js",
        ],
    );
    assert_prior_utilities(&repo, &chain[1], 1, 0..1);

    // sha2 = C3': array_utils.js
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["array_utils.js"]);
    assert_prior_utilities(&repo, &chain[2], 2, 0..2);

    // sha3 = C4': object_utils.js
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["object_utils.js"]);
    assert_prior_utilities(&repo, &chain[3], 3, 0..3);

    // sha4 = C5': number_utils.js
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["number_utils.js"]);
    assert_prior_utilities(&repo, &chain[4], 4, 0..4);

    // sha5 = C6': dom_utils.js
    assert_note_base_commit_matches(&repo, &chain[5], "sha5");
    assert_note_files_exact(&repo, &chain[5], "sha5_files", &["dom_utils.js"]);
    assert_prior_utilities(&repo, &chain[5], 5, 0..5);

    // sha6 = C7': fetch_utils.js
    assert_note_base_commit_matches(&repo, &chain[6], "sha6");
    assert_note_files_exact(&repo, &chain[6], "sha6_files", &["fetch_utils.js"]);
    assert_prior_utilities(&repo, &chain[6], 6, 0..6);

    // sha7 = C8': storage_utils.js
    assert_note_base_commit_matches(&repo, &chain[7], "sha7");
    assert_note_files_exact(&repo, &chain[7], "sha7_files", &["storage_utils.js"]);
    assert_prior_utilities(&repo, &chain[7], 7, 0..7);

    // sha8 = C9': event_utils.js
    assert_note_base_commit_matches(&repo, &chain[8], "sha8");
    assert_note_files_exact(&repo, &chain[8], "sha8_files", &["event_utils.js"]);
    assert_note_no_forbidden_files(&repo, &chain[8], "sha8_no_future", &["validation_utils.js"]);
    assert_prior_utilities(&repo, &chain[8], 8, 0..8);

    // sha9 = C10': validation_utils.js
    assert_note_base_commit_matches(&repo, &chain[9], "sha9");
    assert_note_files_exact(&repo, &chain[9], "sha9_files", &["validation_utils.js"]);
    assert_blame_at_commit(
        &repo,
        &chain[9],
        "validation_utils.js",
        "sha9_blame",
        &[
            ("isEmail", true),
            ("isURL", true),
            ("isPhone", true),
            ("isUUID", true),
            ("minLength", true),
            ("maxLength", true),
            ("required", true),
            ("validate", true),
        ],
    );
    // Verify C1's file (date_utils.js) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[9],
        "date_utils.js",
        "sha9_date_preserved",
        &[
            ("export function formatDate", true),
            ("export function addDays", true),
            ("export function isWeekend", true),
        ],
    );
    assert_prior_utilities(&repo, &chain[9], 9, 1..9);
}

crate::reuse_tests_in_worktree!(test_fast_path_10_commits_javascript_utilities,);
