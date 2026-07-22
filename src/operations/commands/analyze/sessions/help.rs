//! Help text for `git-ai analyze sessions`.

pub(super) fn print_help() {
    let help = r#"git-ai analyze sessions - pull coding sessions into a scratch DB and grade them

Subcommands:
  pull [flags]            Create/fill a SQLite DB with sessions (latest-first)
  next <db> [flags]       Return the next session + transcript and advance the cursor +1
  stats <db>              Show pull progress and where the analysis cursor is
  reset <db> [--to N]     Rewind the analysis cursor (default 0) to re-run from the start
  exec <db> "<SQL>"       Run SQL against the DB (writeback for analysis columns)

pull flags:
  --db <path>     Target DB (default: a fresh /tmp/git-ai-analysis-NNN.db, printed to stdout)
  --limit <n>     Max sessions to pull (default 100)
  --since <range> Cube dateRange on session_start_time (e.g. "last 30 days")
  --repo <url>    Filter by repo_url (convenience equals-filter)
  --user <id>     Filter by user_id (convenience equals-filter)
  --agent <name>  Filter by agent, e.g. claude-code, cursor (convenience equals-filter)
  --include-subagents  Include subagent sessions (default: top-level sessions only)
  --no-enrich     Skip the per-batch tokens/cost/models/PR backfill (faster, fewer columns)

Analyzing your OWN (or one person's) sessions ("my sessions", "what I did"):
  user_id is opaque — resolve it from an email first, then pull with --user:
    EMAIL=$(git config user.email)
    UID=$(git-ai analyze query --tsv -d public_v1_user_status.user_id \
      -f '[{"member":"public_v1_user_status.author_email","operator":"equals","values":["'"$EMAIL"'"]}]' \
      | tail -n +2 | head -1)
    DB=$(git-ai analyze sessions pull --user "$UID" --since "last 30 days")
  Swap in a different email for someone else. If the lookup is empty, stop and say so.

  Full query surface — same flags as `analyze query`, layered on top of the
  canonical session columns so you can slice the population on ANY member:
  -f, --filters '<json>'    Raw Cube filter array (combines with --repo/--user/--agent)
  -d, --dimensions a,b      Extra dimensions to select alongside the session columns
  -m, --measures a,b        Extra measures to select
      --time-dimension M    Time dimension member (default: session_start_time with --since)
  -g, --granularity G       Granularity for the time dimension
  -o, --order M[:asc|desc]  Order rows (default: session_start_time:desc)

  Examples:
    # only sessions that generated >100 net lines, made with cursor:
    git-ai analyze sessions pull --agent cursor \
      -f '[{"member":"public_v1_sessions.net_generated_lines","operator":"gt","values":["100"]}]'
    # sessions whose work reached production, newest committed first:
    git-ai analyze sessions pull \
      -f '[{"member":"public_v1_sessions.total_production_lines","operator":"gt","values":["0"]}]' \
      -o public_v1_sessions.session_end_time:desc

next flags:
  --max-events <n>   Cap transcript events fetched (default 2000)
  --no-transcript    Return session metadata only (no transcript fetch)

What `sessions next` prints (one JSON object, or {"done": true} when exhausted):
  All the `sessions` columns (session_id, agent, repo_url, generated_lines,
  committed_lines, total_events, models, cost_usd, pr_count, …), plus:
    "prs":        array of the PRs this session appears in (repo_url, pr_number,
                  agent, model_raw, ai_lines)
    "transcript": array of events in chronological order — THE PROMPTS + ACTIONS.
  Each transcript event is a flat object; which fields are set depends on event_kind:
    output_seq    int, ordering within an event_time bucket
    event_time    unix seconds
    event_kind    the message type. The transcript is an interleaved, ordered
                  stream dominated by three kinds:
                    user_message       a human turn (the PROMPT) — has .text
                    assistant_message  the agent's reply prose      — has .text
                    tool_call          an action the agent took — has .tool/.tool_kind/
                                       .target/.tool_input/.tool_output
                  and occasionally: skill_load | skill_mention | skill_invoked |
                  pr_created | context_compacted | turn_aborted | session_model_change
    text          message body (user_message / assistant_message)
    summary       short summary when present
    model         model in effect for the event
    tool          tool name for tool_call (e.g. Edit, Read, Bash, Grep)
    tool_kind     shell | file_read | file_edit | web_search | sub_agent | mcp | other
    target        file/path or other target the tool acted on
    tool_input    arguments passed to the tool (string)
    tool_output   result returned by the tool (string)
  So a user's PROMPTS are the event_kind='user_message' .text values, in order; the
  agent's work is the tool_call / assistant_message stream between them. You do NOT
  need to pull a sample session to learn the shape — it is exactly the above.

reset flags:
  --to <seq>         Cursor position to rewind to (default 0 = re-serve everything)

The work funnel (already columns — DO NOT read transcripts to derive these):
  Every session carries its full delivery funnel as plain columns, in order:
    committed_lines → pr_opened_lines → merged_lines → production_lines
  plus the stage-to-stage GAPS, precomputed so you never hand-derive "what
  didn't ship":
    committed_not_pr_opened    committed locally but never opened in a PR
    pr_opened_not_merged       reached a PR but not merged (open OR torn out)
    merged_not_production      merged but not in production
    committed_not_production   headline: committed that never shipped
    production_rate            share of committed lines that reached production
  So "how much did this session commit that didn't ship, and where did it fall
  out?" is a SELECT, not a transcript read:
    git-ai analyze sessions exec "$DB" \
      "SELECT agent, SUM(committed_not_production), AVG(production_rate) \
       FROM sessions GROUP BY agent ORDER BY 2 DESC"
  What these columns CANNOT tell you is the *why* behind a gap — still-open PR
  vs. reverted-as-bad vs. superseded-by-a-rewrite. That distinction lives only
  in the session narrative, so reach for the transcript ONLY for the why, after
  the funnel columns have told you which sessions leak and by how much.

Baseline schema (every pulled session carries these, no extra work):
  sessions columns include the canonical metrics — agent, generated_lines,
  committed_lines, merged_lines, production_lines, total_events (# turns),
  usage_minutes — PLUS the cross-cube baseline backfilled at pull time:
    models                                     comma-joined distinct models used
    input_tokens, output_tokens,               per-session token usage
      cache_read_tokens, cache_creation_tokens,
      reasoning_tokens
    cost_usd                                   per-session estimated cost
    pr_count                                   # of PRs this session appears in
  Two child tables (foreign-keyed on session_id) hold the detail to JOIN against:
    session_models(session_id, model, event_count)
    session_prs(session_id, repo_url, pr_number, agent, model_raw, ai_lines)
  So you can aggregate immediately, e.g.:
    git-ai analyze sessions exec "$DB" \
      "SELECT agent, SUM(cost_usd), AVG(total_events) FROM sessions GROUP BY agent"
    git-ai analyze sessions exec "$DB" \
      "SELECT model, COUNT(*) FROM session_models GROUP BY model ORDER BY 2 DESC"
  (`sessions next` also returns a "prs" array per session.)

How the analysis cursor works:
  Each session row has an auto-increment seq_id. `next` advances a single cursor
  by one and returns that row — atomically, so every row is handed out EXACTLY
  ONCE across any number of concurrent subagents. Nothing to mark "done": once a
  row is served the cursor has already moved past it. Start a new analysis pass
  with `reset` (cursor → 0). `pull` only adds rows; it never moves the cursor.

Grading workflow:
  1. Pull the sessions:
       DB=$(git-ai analyze sessions pull --limit 100 --since "last 30 days")

  2. DESIGN YOUR ANALYSIS SCHEMA FIRST. Turn the user's question into concrete
     columns and ADD them up front — do not just print findings to chat, persist
     them so they can be aggregated. Decide your own criteria for the task:
       - Grading? Derive the rubric, then make a column per criterion plus an
         overall, e.g. clarity_score, correctness_score, grade, rationale.
       - Categorizing? One column for the label plus a notes/evidence column.
     Add them once (TEXT or INTEGER), before iterating:
       git-ai analyze sessions exec "$DB" "ALTER TABLE sessions ADD COLUMN grade TEXT"
       git-ai analyze sessions exec "$DB" "ALTER TABLE sessions ADD COLUMN clarity_score INTEGER"
       git-ai analyze sessions exec "$DB" "ALTER TABLE sessions ADD COLUMN rationale TEXT"

  3. Dispatch N subagents. Each loops, and MUST write its results back into the
     columns you added (not just return prose). The session_id comes from the
     JSON that `next` prints:
       git-ai analyze sessions next "$DB"          # returns one session + transcript JSON
       # …analyze the transcript against your criteria…
       git-ai analyze sessions exec "$DB" \
         "UPDATE sessions SET grade='A', clarity_score=4, rationale='…' \
          WHERE session_id='<id>'"
     Loop until `next` prints {"done": true}.

  4. Watch progress:  git-ai analyze sessions stats "$DB"   # shows cursor position
  5. Synthesize over the enriched columns:
       git-ai analyze sessions exec "$DB" \
         "SELECT grade, COUNT(*), AVG(clarity_score) FROM sessions \
          WHERE grade IS NOT NULL GROUP BY grade ORDER BY grade"
"#;
    eprint!("{help}");
}
