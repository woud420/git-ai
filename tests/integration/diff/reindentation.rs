use super::{DiffLine, TestRepo, fs, parse_diff_output};

/// Regression test: when AI removes wrapper components and re-indents code,
/// lines that happen to be textually identical between old and new (e.g. empty lines)
/// should still be attributed to AI — not show [no-data].
///
/// Uses the actual content from the bug report: a large React component where AI
/// removes header/meter sections and re-indents the grid. Empty lines between
/// motion.div blocks are byte-for-byte identical in old and new, causing imara_diff
/// to treat them as Equal — preserving "human" attribution that then gets stripped.
#[test]
fn test_diff_ai_reindented_lines_attributed_to_ai() {
    let repo = TestRepo::new();

    // This is the actual content from the user's bug report (component.tsx).
    // The old content has wrapper divs with headers/meters before the grid.
    // The AI removes the header/meters and re-indents the grid section,
    // but empty lines between motion.div blocks stay identical.
    let old_content = r##"import React from "react";
import { motion } from "framer-motion";
import {
  Code2,
  Star,
  Zap,
  Cpu,
  Sparkles,
  ChevronRight,
} from "lucide-react";

type LanguageCardProps = {
  name?: string;
  tagline?: string;
  description?: string;
  rank?: number;
  popularity?: number;
  speed?: number;
  vibes?: number;
  colorFrom?: string;
  colorTo?: string;
  icon?: React.ReactNode;
};

function Meter({
  label,
  value,
}: {
  label: string;
  value: number;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between text-sm">
        <span className="text-white/70">{label}</span>
        <span className="font-semibold text-white">{value}%</span>
      </div>
      <div className="h-3 overflow-hidden rounded-full bg-white/10 backdrop-blur">
        <motion.div
          initial={{ width: 0 }}
          animate={{ width: `${value}%` }}
          transition={{ duration: 1, ease: "easeOut" }}
          className="h-full rounded-full bg-gradient-to-r from-white via-white/80 to-white/50"
        />
      </div>
    </div>
  );
}

export default function ExtraLanguageCard({
  name = "TypeScript",
  tagline = "Strongly typed. Ridiculously stylish.",
  description = "A glamorous language card component for your portfolio, dashboard, or devtools UI. Because plain cards are for mortals.",
  rank = 1,
  popularity = 96,
  speed = 84,
  vibes = 100,
  colorFrom = "#7c3aed",
  colorTo = "#06b6d4",
  icon = <Code2 className="h-8 w-8" />,
}: LanguageCardProps) {
  return (
    <div className="min-h-screen bg-[#070b17] px-6 py-12 text-white">
      <div className="mx-auto max-w-5xl">
        <motion.div
          initial={{ opacity: 0, y: 24, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ duration: 0.6 }}
          className="relative overflow-hidden rounded-[32px] border border-white/10 bg-white/5 shadow-2xl backdrop-blur-xl"
        >
          {/* Background glow */}
          <div
            className="absolute inset-0 opacity-90"
            style={{
              background: `
                radial-gradient(circle at top left, ${colorFrom}55 0%, transparent 35%),
                radial-gradient(circle at bottom right, ${colorTo}55 0%, transparent 40%),
                linear-gradient(135deg, ${colorFrom}22, ${colorTo}22)
              `,
            }}
          />

          {/* Floating decorations */}
          <motion.div
            animate={{ y: [0, -10, 0], rotate: [0, 4, 0] }}
            transition={{ repeat: Infinity, duration: 5, ease: "easeInOut" }}
            className="absolute right-8 top-8 rounded-2xl border border-white/10 bg-white/10 p-3 backdrop-blur-md"
          >
            <Sparkles className="h-6 w-6 text-white/90" />
          </motion.div>

          <motion.div
            animate={{ y: [0, 12, 0], rotate: [0, -5, 0] }}
            transition={{ repeat: Infinity, duration: 6, ease: "easeInOut" }}
            className="absolute bottom-10 left-10 rounded-full border border-white/10 bg-white/10 p-4 backdrop-blur-md"
          >
            <Zap className="h-5 w-5 text-white/90" />
          </motion.div>

          <div className="relative z-10 grid gap-8 p-8 md:grid-cols-[1.3fr_0.9fr] md:p-10">
            {/* Left side */}
            <div className="space-y-6">
              <div className="flex flex-wrap items-center gap-3">
                <span className="inline-flex items-center gap-2 rounded-full border border-white/15 bg-white/10 px-4 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-white/80">
                  <Star className="h-4 w-4" />
                  Featured Language
                </span>

                <span className="inline-flex items-center rounded-full bg-white px-3 py-1 text-xs font-black text-slate-900">
                  #{rank} Trending
                </span>
              </div>

              <div className="flex items-start gap-4">
                <div
                  className="rounded-[24px] border border-white/15 p-4 shadow-xl"
                  style={{
                    background: `linear-gradient(135deg, ${colorFrom}, ${colorTo})`,
                  }}
                >
                  {icon}
                </div>

                <div>
                  <h1 className="text-4xl font-black tracking-tight md:text-6xl">
                    {name}
                  </h1>
                  <p className="mt-2 text-lg text-white/75 md:text-xl">
                    {tagline}
                  </p>
                </div>
              </div>

              <p className="max-w-2xl text-base leading-7 text-white/80 md:text-lg">
                {description}
              </p>

              <div className="flex flex-wrap gap-3">
                {["Type Safe", "Modern DX", "Production Ready", "Elite Vibes"].map(
                  (badge) => (
                    <motion.span
                      key={badge}
                      whileHover={{ scale: 1.06, y: -2 }}
                      className="rounded-full border border-white/15 bg-white/10 px-4 py-2 text-sm font-medium text-white/90 backdrop-blur"
                    >
                      {badge}
                    </motion.span>
                  )
                )}
              </div>

              <div className="flex flex-wrap gap-4 pt-2">
                <motion.button
                  whileHover={{ scale: 1.04 }}
                  whileTap={{ scale: 0.98 }}
                  className="group inline-flex items-center gap-2 rounded-2xl bg-white px-5 py-3 font-bold text-slate-900 shadow-xl"
                >
                  Explore Language
                  <ChevronRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
                </motion.button>

                <motion.button
                  whileHover={{ scale: 1.04 }}
                  whileTap={{ scale: 0.98 }}
                  className="inline-flex items-center gap-2 rounded-2xl border border-white/15 bg-white/10 px-5 py-3 font-semibold text-white backdrop-blur"
                >
                  <Cpu className="h-4 w-4" />
                  Compare Stats
                </motion.button>
              </div>
            </div>

            {/* Right side */}
            <div className="space-y-5 rounded-[28px] border border-white/10 bg-black/20 p-6 backdrop-blur-xl">
              <div className="flex items-center justify-between">
                <h2 className="text-xl font-bold">Power Metrics</h2>
                <span className="rounded-full border border-emerald-400/20 bg-emerald-400/10 px-3 py-1 text-xs font-semibold text-emerald-300">
                  MAXED OUT
                </span>
              </div>

              <Meter label="Popularity" value={popularity} />
              <Meter label="Performance" value={speed} />
              <Meter label="Developer Vibes" value={vibes} />

              <div className="grid grid-cols-2 gap-4 pt-4">
                <motion.div
                  whileHover={{ y: -4 }}
                  className="rounded-2xl border border-white/10 bg-white/5 p-4"
                >
                  <div className="text-sm text-white/65">Ecosystem</div>
                  <div className="mt-2 text-2xl font-black">Huge</div>
                </motion.div>

                <motion.div
                  whileHover={{ y: -4 }}
                  className="rounded-2xl border border-white/10 bg-white/5 p-4"
                >
                  <div className="text-sm text-white/65">Learning Curve</div>
                  <div className="mt-2 text-2xl font-black">Smooth-ish</div>
                </motion.div>

                <motion.div
                  whileHover={{ y: -4 }}
                  className="rounded-2xl border border-white/10 bg-white/5 p-4"
                >
                  <div className="text-sm text-white/65">Use Case</div>
                  <div className="mt-2 text-2xl font-black">Everything</div>
                </motion.div>

                <motion.div
                  whileHover={{ y: -4 }}
                  className="rounded-2xl border border-white/10 bg-white/5 p-4"
                >
                  <div className="text-sm text-white/65">Aura</div>
                  <div className="mt-2 text-2xl font-black">Legendary</div>
                </motion.div>
              </div>
            </div>
          </div>

          {/* Bottom shine */}
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-white/10 to-transparent" />
        </motion.div>
      </div>
    </div>
  );
}
"##;

    // New content: AI removed header/meters, kept the grid section but re-indented.
    // Empty lines between motion.div blocks are byte-for-byte identical to old content.
    let new_content = r##"import React from "react";
import { motion } from "framer-motion";
import {
  Code2,
  Star,
  Zap,
  Cpu,
  Sparkles,
  ChevronRight,
} from "lucide-react";

type LanguageCardProps = {
  name?: string;
  tagline?: string;
  description?: string;
  rank?: number;
  popularity?: number;
  speed?: number;
  vibes?: number;
  colorFrom?: string;
  colorTo?: string;
  icon?: React.ReactNode;
};

function Meter({
  label,
  value,
}: {
  label: string;
  value: number;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between text-sm">
        <span className="text-white/70">{label}</span>
        <span className="font-semibold text-white">{value}%</span>
      </div>
      <div className="h-3 overflow-hidden rounded-full bg-white/10 backdrop-blur">
        <motion.div
          initial={{ width: 0 }}
          animate={{ width: `${value}%` }}
          transition={{ duration: 1, ease: "easeOut" }}
          className="h-full rounded-full bg-gradient-to-r from-white via-white/80 to-white/50"
        />
      </div>
    </div>
  );
}

export default function ExtraLanguageCard({
  name = "TypeScript",
  tagline = "Strongly typed. Ridiculously stylish.",
  description = "A glamorous language card component for your portfolio, dashboard, or devtools UI. Because plain cards are for mortals.",
  rank = 1,
  popularity = 96,
  speed = 84,
  vibes = 100,
  colorFrom = "#7c3aed",
  colorTo = "#06b6d4",
  icon = <Code2 className="h-8 w-8" />,
}: LanguageCardProps) {
  return (
    <div className="min-h-screen bg-[#070b17] px-6 py-12 text-white">
      <div className="mx-auto max-w-5xl">
        <motion.div
          initial={{ opacity: 0, y: 24, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ duration: 0.6 }}
          className="relative overflow-hidden rounded-[32px] border border-white/10 bg-white/5 shadow-2xl backdrop-blur-xl"
        >
          {/* Background glow */}
          <div
            className="absolute inset-0 opacity-90"
            style={{
              background: `
                radial-gradient(circle at top left, ${colorFrom}55 0%, transparent 35%),
                radial-gradient(circle at bottom right, ${colorTo}55 0%, transparent 40%),
                linear-gradient(135deg, ${colorFrom}22, ${colorTo}22)
              `,
            }}
          />

          {/* Floating decorations */}
          <motion.div
            animate={{ y: [0, -10, 0], rotate: [0, 4, 0] }}
            transition={{ repeat: Infinity, duration: 5, ease: "easeInOut" }}
            className="absolute right-8 top-8 rounded-2xl border border-white/10 bg-white/10 p-3 backdrop-blur-md"
          >
            <Sparkles className="h-6 w-6 text-white/90" />
          </motion.div>

          <motion.div
            animate={{ y: [0, 12, 0], rotate: [0, -5, 0] }}
            transition={{ repeat: Infinity, duration: 6, ease: "easeInOut" }}
            className="absolute bottom-10 left-10 rounded-full border border-white/10 bg-white/10 p-4 backdrop-blur-md"
          >
            <Zap className="h-5 w-5 text-white/90" />
          </motion.div>

          <div className="relative z-10 grid gap-8 p-8 md:grid-cols-[1.3fr_0.9fr] md:p-10">
            {/* Left side */}
            <div className="space-y-6">
              <div className="flex flex-wrap items-center gap-3">
                <span className="inline-flex items-center gap-2 rounded-full border border-white/15 bg-white/10 px-4 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-white/80">
                  <Star className="h-4 w-4" />
                  Featured Language
                </span>

                <span className="inline-flex items-center rounded-full bg-white px-3 py-1 text-xs font-black text-slate-900">
                  #{rank} Trending
                </span>
              </div>

              <div className="flex items-start gap-4">
                <div
                  className="rounded-[24px] border border-white/15 p-4 shadow-xl"
                  style={{
                    background: `linear-gradient(135deg, ${colorFrom}, ${colorTo})`,
                  }}
                >
                  {icon}
                </div>

                <div>
                  <h1 className="text-4xl font-black tracking-tight md:text-6xl">
                    {name}
                  </h1>
                  <p className="mt-2 text-lg text-white/75 md:text-xl">
                    {tagline}
                  </p>
                </div>
              </div>

              <p className="max-w-2xl text-base leading-7 text-white/80 md:text-lg">
                {description}
              </p>

              <div className="flex flex-wrap gap-3">
                {["Type Safe", "Modern DX", "Production Ready", "Elite Vibes"].map(
                  (badge) => (
                    <motion.span
                      key={badge}
                      whileHover={{ scale: 1.06, y: -2 }}
                      className="rounded-full border border-white/15 bg-white/10 px-4 py-2 text-sm font-medium text-white/90 backdrop-blur"
                    >
                      {badge}
                    </motion.span>
                  )
                )}
              </div>

              <div className="flex flex-wrap gap-4 pt-2">
                <motion.button
                  whileHover={{ scale: 1.04 }}
                  whileTap={{ scale: 0.98 }}
                  className="group inline-flex items-center gap-2 rounded-2xl bg-white px-5 py-3 font-bold text-slate-900 shadow-xl"
                >
                  Explore Language
                  <ChevronRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
                </motion.button>

                <motion.button
                  whileHover={{ scale: 1.04 }}
                  whileTap={{ scale: 0.98 }}
                  className="inline-flex items-center gap-2 rounded-2xl border border-white/15 bg-white/10 px-5 py-3 font-semibold text-white backdrop-blur"
                >
                  <Cpu className="h-4 w-4" />
                  Compare Stats
                </motion.button>
              </div>
            </div>

            {/* Right side */}
            <div className="space-y-5 rounded-[28px] border border-white/10 bg-black/20 p-6 backdrop-blur-xl">
            <div className="grid grid-cols-2 gap-4 pt-4">
              <motion.div
                whileHover={{ y: -4 }}
                className="rounded-2xl border border-white/10 bg-white/5 p-4"
              >
                <div className="text-sm text-white/65">Ecosystem</div>
                <div className="mt-2 text-2xl font-black">Huge</div>
              </motion.div>

              <motion.div
                whileHover={{ y: -4 }}
                className="rounded-2xl border border-white/10 bg-white/5 p-4"
              >
                <div className="text-sm text-white/65">Learning Curve</div>
                <div className="mt-2 text-2xl font-black">Smooth-ish</div>
              </motion.div>

              <motion.div
                whileHover={{ y: -4 }}
                className="rounded-2xl border border-white/10 bg-white/5 p-4"
              >
                <div className="text-sm text-white/65">Use Case</div>
                <div className="mt-2 text-2xl font-black">Everything</div>
              </motion.div>

              <motion.div
                whileHover={{ y: -4 }}
                className="rounded-2xl border border-white/10 bg-white/5 p-4"
              >
                <div className="text-sm text-white/65">Aura</div>
                <div className="mt-2 text-2xl font-black">Legendary</div>
              </motion.div>
            </div>
            </div>
          </div>

          {/* Bottom shine */}
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-white/10 to-transparent" />
        </motion.div>
      </div>
    </div>
  );
}
"##;

    // Step 1: write old content and commit (initial human commit)
    let file_path = "component.tsx";
    let full_path = repo.path().join(file_path);
    fs::write(&full_path, old_content).expect("write old content");
    repo.git(&["add", file_path]).expect("git add");
    repo.git_og(&["commit", "-m", "initial"])
        .expect("initial commit");

    // Step 2: AI makes changes — write new content and checkpoint as AI
    fs::write(&full_path, new_content).expect("write new content");
    repo.git_ai(&["checkpoint", "mock_ai", file_path])
        .expect("checkpoint should succeed");

    // Step 3: commit
    repo.git(&["add", file_path]).expect("git add");
    let commit = repo.commit("ai refactor").expect("commit should succeed");

    // Step 4: run git ai diff and verify ALL added lines are attributed to AI
    let diff_output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git ai diff should succeed");

    let diff_lines = parse_diff_output(&diff_output);

    // Every added line (prefix "+") should be attributed to AI (ai:mock_ai),
    // not [no-data]. Deleted lines (prefix "-") don't need attribution.
    let added_lines: Vec<&DiffLine> = diff_lines.iter().filter(|l| l.prefix == "+").collect();

    assert!(
        !added_lines.is_empty(),
        "Expected added lines in diff output, got none.\nFull diff:\n{}",
        diff_output
    );

    let no_data_lines: Vec<&&DiffLine> = added_lines
        .iter()
        .filter(|l| l.attribution.as_deref() == Some("no-data"))
        .collect();

    assert!(
        no_data_lines.is_empty(),
        "Found {} added lines with [no-data] attribution that should be attributed to AI:\n{}\nFull diff:\n{}",
        no_data_lines.len(),
        no_data_lines
            .iter()
            .map(|l| format!("  +{} [no-data]", l.content))
            .collect::<Vec<_>>()
            .join("\n"),
        diff_output
    );

    // Additionally verify that all added lines have ai:mock_ai attribution
    for line in &added_lines {
        assert!(
            line.attribution
                .as_ref()
                .is_some_and(|a| a.contains("ai:mock_ai")),
            "Added line should be attributed to mock_ai, but got {:?}: content='{}'\nFull diff:\n{}",
            line.attribution,
            line.content,
            diff_output
        );
    }
}

crate::reuse_tests_in_worktree!(test_diff_ai_reindented_lines_attributed_to_ai,);
