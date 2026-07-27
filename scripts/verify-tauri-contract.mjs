import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");

function read(relativePath) {
  return fs.readFileSync(path.join(root, relativePath), "utf8");
}

function walk(directory, extension) {
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) return walk(fullPath, extension);
      return entry.name.endsWith(extension) ? [fullPath] : [];
    });
}

function unique(values) {
  return [...new Set(values)].sort();
}

function extractStringCalls(source, callees) {
  const values = [];
  for (const callee of callees) {
    let cursor = 0;
    while ((cursor = source.indexOf(callee, cursor)) >= 0) {
      let index = cursor + callee.length;
      while (/\s/.test(source[index] ?? "")) index += 1;
      if (source[index] === "<") {
        let depth = 0;
        do {
          if (source[index] === "<") depth += 1;
          if (source[index] === ">") depth -= 1;
          index += 1;
        } while (index < source.length && depth > 0);
        while (/\s/.test(source[index] ?? "")) index += 1;
      }
      if (source[index] !== "(") {
        cursor = index;
        continue;
      }
      index += 1;
      while (/\s/.test(source[index] ?? "")) index += 1;
      const quote = source[index];
      if (quote !== '"' && quote !== "'") {
        cursor = index;
        continue;
      }
      const end = source.indexOf(quote, index + 1);
      if (end > index + 1) values.push(source.slice(index + 1, end));
      cursor = end > index ? end + 1 : index + 1;
    }
  }
  return values;
}

const apiSource = read("src/lib/api.ts");
const tauriLibSource = read("src-tauri/src/lib.rs");
const frontendEventSource = walk(path.join(root, "src"), ".tsx")
  .map((file) => fs.readFileSync(file, "utf8"))
  .join("\n");
const rustEventSource = walk(path.join(root, "src-tauri", "src"), ".rs")
  .map((file) => fs.readFileSync(file, "utf8"))
  .join("\n");

const invokedCommands = unique(
  extractStringCalls(apiSource, ["invoke"]).filter((value) =>
    /^[a-z][a-z0-9_]*$/.test(value),
  ),
);
const registeredCommands = new Set(
  [...tauriLibSource.matchAll(/(?:commands|tray)::([a-z][a-z0-9_]*)/g)]
    .map((match) => match[1]),
);
const listenedEvents = unique(
  extractStringCalls(frontendEventSource, ["api.listen", "listen"]).filter(
    (value) => value.includes("://"),
  ),
);
const emittedEvents = new Set(
  [...rustEventSource.matchAll(/\.emit\s*\(\s*["']([^"']+)["']/g)]
    .map((match) => match[1]),
);

const missingCommands = invokedCommands.filter(
  (command) => !registeredCommands.has(command),
);
const missingEvents = listenedEvents.filter((event) => !emittedEvents.has(event));

if (invokedCommands.length === 0 || listenedEvents.length === 0) {
  throw new Error("Contract scan found no commands or events; update the scanner.");
}

if (missingCommands.length || missingEvents.length) {
  if (missingCommands.length) {
    console.error(
      `Frontend invokes commands not registered by Tauri: ${missingCommands.join(", ")}`,
    );
  }
  if (missingEvents.length) {
    console.error(
      `Frontend listens to events not emitted by the Host: ${missingEvents.join(", ")}`,
    );
  }
  process.exitCode = 1;
} else {
  console.log(
    `Tauri contract OK: ${invokedCommands.length} commands, ${listenedEvents.length} events.`,
  );
}
