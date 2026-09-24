import { readFile } from "node:fs/promises";
import satisfies from "spdx-satisfies";

// This is an inventory gate, not a substitute for retaining upstream notices.
// Every dependency, including build tools and optional platform packages, is
// checked. Missing, invalid, or newly introduced license terms require review.
const allowed = [
  "MIT",
  "MIT-0",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "0BSD",
  "CC0-1.0",
  "CC-BY-3.0",
  "CC-BY-4.0",
  "BlueOak-1.0.0",
];

// Existing tool-only packages with distinct terms are reviewed individually.
// A move into runtime dependencies or a license change must be reviewed again.
const developmentExceptions = new Map([
  ["node_modules/argparse", ["Python-2.0"]],
  ["node_modules/axe-core", ["MPL-2.0"]],
]);

const lock = JSON.parse(await readFile(new URL("../package-lock.json", import.meta.url), "utf8"));
const failures = [];
let checked = 0;

if (!lock.packages || lock.lockfileVersion < 2) {
  throw new Error(
    "License checks require an npm lockfile with package metadata (version 2 or later).",
  );
}

for (const [path, entry] of Object.entries(lock.packages)) {
  if (path === "") continue; // The application's own license is maintained separately.
  checked += 1;
  try {
    const packageLicenses = entry.dev
      ? [...allowed, ...(developmentExceptions.get(path) ?? [])]
      : allowed;
    if (typeof entry.license !== "string" || !satisfies(entry.license, packageLicenses)) {
      failures.push(`${path}@${entry.version}: ${entry.license ?? "missing license metadata"}`);
    }
  } catch {
    failures.push(
      `${path}@${entry.version}: invalid SPDX expression ${JSON.stringify(entry.license)}`,
    );
  }
}

if (failures.length > 0) {
  console.error(
    "Dependency licenses need review:\n" + failures.map((failure) => `- ${failure}`).join("\n"),
  );
  process.exitCode = 1;
} else {
  console.log(`License metadata checked for ${checked} npm dependencies.`);
}
