import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const policy = await readJson(path.join(root, "coverage-policy.json"));
const lcov = await readFile(
  path.join(root, ".cache", "rust-coverage", "lcov.info"),
  "utf8",
);
const mode = process.argv[2] ?? "baseline";
const totals = sumLcov(lcov);
const lines = percentage(totals.lines);
const branches = percentage(totals.branches);
const failures = [];

if (mode === "baseline") {
  const minimum =
    policy.rust.baseline.lines.pct -
    policy.baselineGate.maxRegressionPercentagePoints;
  console.log(
    `Rust line coverage: ${formatPct(lines)} (${totals.lines.covered}/${totals.lines.total}), minimum ${formatPct(minimum)}`,
  );
  if (lines < minimum) failures.push("Rust line coverage regressed.");
} else if (mode === "final") {
  console.log(
    `Rust line coverage: ${formatPct(lines)}, final target ${formatPct(policy.rust.finalTarget.lines)}`,
  );
  if (lines < policy.rust.finalTarget.lines) {
    failures.push("Rust line coverage is below the final target.");
  }
  if (totals.branches.total === 0) {
    failures.push(
      `Rust branch coverage was not collected. ${policy.rust.branchCollection}`,
    );
  } else {
    console.log(
      `Rust branch coverage: ${formatPct(branches)}, final target ${formatPct(policy.rust.finalTarget.branches)}`,
    );
    if (branches < policy.rust.finalTarget.branches) {
      failures.push("Rust branch coverage is below the final target.");
    }
  }
} else {
  failures.push(`Unknown Rust coverage audit mode: ${mode}`);
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}

function sumLcov(source) {
  const totals = {
    lines: { total: 0, covered: 0 },
    branches: { total: 0, covered: 0 },
  };
  for (const line of source.split("\n")) {
    if (line.startsWith("LF:")) totals.lines.total += Number(line.slice(3));
    else if (line.startsWith("LH:")) {
      totals.lines.covered += Number(line.slice(3));
    } else if (line.startsWith("BRF:")) {
      totals.branches.total += Number(line.slice(4));
    } else if (line.startsWith("BRH:")) {
      totals.branches.covered += Number(line.slice(4));
    }
  }
  return totals;
}

function percentage({ covered, total }) {
  return total === 0 ? 0 : (covered / total) * 100;
}

function formatPct(value) {
  return `${value.toFixed(2)}%`;
}

async function readJson(filename) {
  return JSON.parse(await readFile(filename, "utf8"));
}
