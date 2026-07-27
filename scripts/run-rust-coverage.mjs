import { mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outputDirectory = path.join(root, ".cache", "rust-coverage");
const outputFile = path.join(outputDirectory, "lcov.info");

mkdirSync(outputDirectory, { recursive: true });

run([
  "llvm-cov",
  "--manifest-path",
  path.join(root, "src-tauri", "Cargo.toml"),
  "--workspace",
  "--lcov",
  "--output-path",
  outputFile,
]);
run([
  "llvm-cov",
  "--manifest-path",
  path.join(root, "src-tauri", "Cargo.toml"),
  "report",
  "--summary-only",
]);

function run(args) {
  const result = spawnSync("cargo", args, {
    cwd: root,
    stdio: "inherit",
  });
  if (result.error) {
    console.error(
      "cargo-llvm-cov is required. Install it with `cargo install cargo-llvm-cov`.",
    );
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}
