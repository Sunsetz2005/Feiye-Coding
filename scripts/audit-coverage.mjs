import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const policy = await readJson(path.join(root, "coverage-policy.json"));
const mode = process.argv[2] ?? "baseline";

if (mode === "baseline" || mode === "final") {
  await auditTotals(mode);
} else if (mode === "changed") {
  await auditChangedCode();
} else {
  fail(`Unknown coverage audit mode: ${mode}`);
}

async function auditTotals(auditMode) {
  const report = await readJson(path.join(root, policy.reports.summary));
  const expected =
    auditMode === "baseline"
      ? Object.fromEntries(
          Object.entries(policy.baseline.metrics).map(([metric, value]) => [
            metric,
            value.pct - policy.baselineGate.maxRegressionPercentagePoints,
          ]),
        )
      : policy.finalTarget;
  const failures = [];

  console.log(
    auditMode === "baseline"
      ? `Coverage baseline audit (tolerance ${policy.baselineGate.maxRegressionPercentagePoints} percentage points)`
      : "Final coverage target audit",
  );
  for (const [metric, minimum] of Object.entries(expected)) {
    const current = report.total?.[metric];
    if (!current || typeof current.pct !== "number") {
      failures.push(`${metric}: missing from ${policy.reports.summary}`);
      continue;
    }
    console.log(
      `  ${metric.padEnd(10)} ${formatPct(current.pct)} (${current.covered}/${current.total}), minimum ${formatPct(minimum)}`,
    );
    if (current.pct + Number.EPSILON < minimum) {
      failures.push(
        `${metric}: ${formatPct(current.pct)} is below ${formatPct(minimum)}`,
      );
    }
  }

  if (failures.length > 0) {
    fail(failures.join("\n"));
  }
}

async function auditChangedCode() {
  const detail = await readJson(path.join(root, policy.reports.detail));
  const base = readOption("--base") ?? process.env.COVERAGE_BASE;
  const changed = changedSourceLines(base);
  const coverageByFile = new Map(
    Object.entries(detail).map(([filename, coverage]) => [
      normalizePath(path.relative(root, filename)),
      coverage,
    ]),
  );
  const totals = {
    lines: { total: 0, covered: 0 },
    branches: { total: 0, covered: 0 },
  };
  const fileTotals = [];
  const missing = [];

  for (const [filename, lines] of changed) {
    if (!isProductionSource(filename)) continue;
    const coverage = coverageByFile.get(filename);
    if (!coverage) {
      missing.push(filename);
      continue;
    }
    const currentFile = {
      filename,
      lines: { total: 0, covered: 0 },
      branches: { total: 0, covered: 0 },
    };

    for (const [id, location] of Object.entries(coverage.statementMap ?? {})) {
      if (!locationIntersectsChangedLines(location, lines)) continue;
      totals.lines.total += 1;
      currentFile.lines.total += 1;
      if ((coverage.s?.[id] ?? 0) > 0) {
        totals.lines.covered += 1;
        currentFile.lines.covered += 1;
      }
    }
    for (const [id, branch] of Object.entries(coverage.branchMap ?? {})) {
      const locations = [branch.loc, ...(branch.locations ?? [])].filter(
        Boolean,
      );
      const intersects =
        locations.some((location) =>
          locationIntersectsChangedLines(location, lines),
        ) ||
        (typeof branch.line === "number" && lines.has(branch.line));
      if (!intersects) continue;
      for (const count of coverage.b?.[id] ?? []) {
        totals.branches.total += 1;
        currentFile.branches.total += 1;
        if (count > 0) {
          totals.branches.covered += 1;
          currentFile.branches.covered += 1;
        }
      }
    }
    if (currentFile.lines.total > 0 || currentFile.branches.total > 0) {
      fileTotals.push(currentFile);
    }
  }

  if (missing.length > 0) {
    fail(`Changed production files missing from coverage:\n${missing.join("\n")}`);
  }
  if (totals.lines.total === 0) {
    console.log("Changed-code coverage: no changed executable TypeScript lines.");
    return;
  }

  const linePct = percentage(totals.lines);
  const branchPct = percentage(totals.branches);
  const failures = [];
  for (const file of fileTotals.sort((left, right) =>
    left.filename.localeCompare(right.filename),
  )) {
    const branchDetail =
      file.branches.total > 0
        ? `, branches ${formatPct(percentage(file.branches))} (${file.branches.covered}/${file.branches.total})`
        : "";
    console.log(
      `  ${file.filename}: lines ${formatPct(percentage(file.lines))} (${file.lines.covered}/${file.lines.total})${branchDetail}`,
    );
  }
  console.log(
    `Changed-code lines: ${formatPct(linePct)} (${totals.lines.covered}/${totals.lines.total}), minimum ${formatPct(policy.changedCodeGate.lines)}`,
  );
  if (linePct < policy.changedCodeGate.lines) {
    failures.push(
      `lines: ${formatPct(linePct)} is below ${formatPct(policy.changedCodeGate.lines)}`,
    );
  }
  if (totals.branches.total > 0) {
    console.log(
      `Changed-code branches: ${formatPct(branchPct)} (${totals.branches.covered}/${totals.branches.total}), minimum ${formatPct(policy.changedCodeGate.branches)}`,
    );
    if (branchPct < policy.changedCodeGate.branches) {
      failures.push(
        `branches: ${formatPct(branchPct)} is below ${formatPct(policy.changedCodeGate.branches)}`,
      );
    }
  }
  if (failures.length > 0) {
    fail(failures.join("\n"));
  }
}

function changedSourceLines(base) {
  const args = [
    "diff",
    "--unified=0",
    "--no-color",
    "--no-ext-diff",
    "--diff-filter=ACMR",
  ];
  if (base) args.push(`${base}...HEAD`);
  else args.push("HEAD");
  args.push("--", "src");

  const diff = git(args);
  const files = parseDiff(diff);
  if (!base) {
    const untracked = git([
      "ls-files",
      "--others",
      "--exclude-standard",
      "--",
      "src",
    ]);
    for (const filename of untracked.split("\n").filter(Boolean)) {
      const normalized = normalizePath(filename);
      const source = readFileSync(path.join(root, filename), "utf8");
      files.set(
        normalized,
        new Set(Array.from({ length: source.split("\n").length }, (_, i) => i + 1)),
      );
    }
  }
  return files;
}

function parseDiff(diff) {
  const files = new Map();
  let current;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ ")) {
      const name = line.slice(4);
      current =
        name === "/dev/null" ? undefined : normalizePath(name.replace(/^b\//, ""));
      if (current && !files.has(current)) files.set(current, new Set());
      continue;
    }
    const match = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/);
    if (!match || !current) continue;
    const start = Number(match[1]);
    const count = match[2] === undefined ? 1 : Number(match[2]);
    const lines = files.get(current);
    for (let offset = 0; offset < count; offset += 1) {
      lines.add(start + offset);
    }
  }
  return files;
}

function isProductionSource(filename) {
  return (
    /^src\/.+\.(?:ts|tsx)$/.test(filename) &&
    !/\.(?:test|spec)\.(?:ts|tsx)$/.test(filename) &&
    !filename.endsWith(".d.ts")
  );
}

function readOption(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function git(args) {
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  }).trim();
}

function normalizePath(filename) {
  return filename.split(path.sep).join("/");
}

function percentage({ covered, total }) {
  return total === 0 ? 100 : (covered / total) * 100;
}

function locationIntersectsChangedLines(location, changedLines) {
  const start = location?.start?.line;
  const end = location?.end?.line ?? start;
  if (typeof start !== "number" || typeof end !== "number") return false;
  for (const line of changedLines) {
    if (line >= start && line <= end) return true;
  }
  return false;
}

function formatPct(value) {
  return `${value.toFixed(2)}%`;
}

async function readJson(filename) {
  try {
    return JSON.parse(await readFile(filename, "utf8"));
  } catch (error) {
    fail(`Unable to read ${path.relative(root, filename)}: ${error.message}`);
  }
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
