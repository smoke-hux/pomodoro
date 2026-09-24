#!/usr/bin/env node
// Real Linux WebView + Rust backend, using the W3C WebDriver HTTP protocol.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { constants } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(root, process.env.POMODORO_TEST_BINARY ?? "src-tauri/target/debug/pomodoro");
const driver = process.env.TAURI_DRIVER ?? "tauri-driver";
const nativeDriver = process.env.WEBKIT_WEBDRIVER
  ? resolve(process.env.WEBKIT_WEBDRIVER)
  : undefined;
const artifacts = process.env.POMODORO_TEST_ARTIFACTS
  ? resolve(root, process.env.POMODORO_TEST_ARTIFACTS)
  : undefined;
const elementKey = "element-6066-11e4-a52e-4f735466cecf";
let temporary;
let child;
let session;
let endpoint;
let driverLog = "";

function required(command, args, hint) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 5_000 });
  if (result.error || result.status !== 0) throw new Error(`${command} is unavailable. ${hint}`);
}

async function availablePort() {
  const server = createServer();
  await new Promise((accept, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", accept);
  });
  const { port } = server.address();
  await new Promise((accept, reject) =>
    server.close((error) => (error ? reject(error) : accept())),
  );
  return port;
}

async function request(method, path, body) {
  const response = await fetch(`${endpoint}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(20_000),
  });
  const result = await response.json();
  if (!response.ok || result.value?.error) {
    throw new Error(`${method} ${path}: ${result.value?.message ?? response.statusText}`);
  }
  return result.value;
}

function command(method, path, body) {
  return request(method, `/session/${session}${path}`, body);
}

async function until(description, check, timeout = 15_000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    await delay(150);
  }
  throw new Error(`Timed out waiting for ${description}${last ? `: ${last.message}` : ""}`);
}

async function element(selector) {
  const found = await until(selector, () =>
    command("POST", "/element", { using: "css selector", value: selector }),
  );
  return found[elementKey];
}

async function click(selector) {
  const id = await element(selector);
  await command("POST", `/element/${id}/click`, {});
}

async function fill(selector, text) {
  const id = await element(selector);
  await command("POST", `/element/${id}/click`, {});
  // WebDriver's clear command does not reliably emit React's input events in
  // WebKit. Real selection/backspace keys keep the controlled value in sync.
  await command("POST", "/actions", {
    actions: [
      {
        type: "key",
        id: "keyboard",
        actions: [
          { type: "keyDown", value: "\uE009" },
          { type: "keyDown", value: "a" },
          { type: "keyUp", value: "a" },
          { type: "keyUp", value: "\uE009" },
          { type: "keyDown", value: "\uE003" },
          { type: "keyUp", value: "\uE003" },
        ],
      },
    ],
  });
  await command("POST", `/element/${id}/value`, { text, value: [...text] });
  await until(`input value for ${selector}`, () =>
    evaluate(
      "return document.querySelector(arguments[0])?.value === arguments[1];",
      selector,
      text,
    ),
  );
}

function evaluate(script, ...args) {
  return command("POST", "/execute/sync", { script, args });
}

async function screenshot(name) {
  if (!artifacts || !session) return;
  await mkdir(artifacts, { recursive: true });
  const png = await command("GET", "/screenshot");
  await writeFile(join(artifacts, `${name}.png`), Buffer.from(png, "base64"));
}

async function readStore() {
  return JSON.parse(
    await readFile(join(temporary, "data/app.pomodoro.timer/pomodoro.json"), "utf8"),
  );
}

async function state(predicate, description) {
  return until(description, async () => {
    const data = await readStore();
    return predicate(data) && data;
  });
}

async function stop() {
  if (session) {
    await command("DELETE", "").catch(() => {});
    session = undefined;
  }
  if (!child) return;
  const owned = child;
  child = undefined;
  if (!owned.pid) return;
  // Every process in this group was created by this test, including D-Bus,
  // WebKitWebDriver and the app. Closing a window alone can leave it in tray.
  const signalGroup = (signal) => {
    try {
      process.kill(-owned.pid, signal);
      return true;
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
      return false;
    }
  };
  if (signalGroup("SIGTERM")) {
    await delay(300);
    signalGroup("SIGKILL");
  }
}

async function launch() {
  const port = await availablePort();
  const nativePort = await availablePort();
  endpoint = `http://127.0.0.1:${port}`;
  const testEnv = {
    ...process.env,
    XDG_DATA_HOME: join(temporary, "data"),
    XDG_CONFIG_HOME: join(temporary, "config"),
    XDG_CACHE_HOME: join(temporary, "cache"),
    XDG_RUNTIME_DIR: join(temporary, "runtime"),
    GSETTINGS_BACKEND: "memory",
    WEBKIT_DISABLE_DMABUF_RENDERER: "1",
    GDK_BACKEND: "x11",
  };
  // Never use or activate services on the user's desktop session bus.
  delete testEnv.DBUS_SESSION_BUS_ADDRESS;
  delete testEnv.DBUS_SESSION_BUS_PID;
  child = spawn(
    "dbus-run-session",
    [
      "--",
      driver,
      "--port",
      String(port),
      "--native-port",
      String(nativePort),
      ...(nativeDriver ? ["--native-driver", nativeDriver] : []),
    ],
    { env: testEnv, detached: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let startupError;
  child.once("error", (error) => {
    startupError = error;
  });
  const collect = (chunk) => {
    driverLog = (driverLog + chunk.toString()).slice(-64_000);
  };
  child.stdout.on("data", collect);
  child.stderr.on("data", collect);
  await until("WebDriver startup", async () => {
    if (startupError) throw startupError;
    const status = await request("GET", "/status");
    return status.ready;
  });
  const created = await request("POST", "/session", {
    capabilities: { alwaysMatch: { "tauri:options": { application: binary } } },
  });
  session = created.sessionId;
  assert.ok(session, "WebDriver did not return a session ID");
  await element('.app-shell button[aria-label="Open settings"]');
  await command("POST", "/window/rect", { width: 1080, height: 720 });
}

async function smoke() {
  assert.equal(process.platform, "linux", "This desktop smoke test requires Linux.");
  required(driver, ["--help"], "Install with: cargo install tauri-driver --locked");
  required(
    nativeDriver ?? "WebKitWebDriver",
    ["--help"],
    "Install the webkit2gtk-driver OS package.",
  );
  required("dbus-run-session", ["--version"], "Install the dbus-daemon OS package.");
  assert.ok(process.env.DISPLAY, "Run under a virtual display: xvfb-run -a npm run test:desktop");
  await access(binary, constants.X_OK).catch(() => {
    throw new Error(
      `Build the embedded frontend first: npm run tauri -- build --debug --no-bundle (${binary})`,
    );
  });
  temporary = await mkdtemp(join(tmpdir(), "pomodoro-desktop-smoke-"));
  for (const name of ["data/app.pomodoro.timer", "config", "cache", "runtime"]) {
    await mkdir(join(temporary, name), { recursive: true, mode: 0o700 });
  }
  const storePath = join(temporary, "data/app.pomodoro.timer/pomodoro.json");
  // An intentionally small legacy store also exercises upgrade compatibility.
  // The UI supplies the timer duration; no test-only IPC or time travel is used.
  await writeFile(
    storePath,
    JSON.stringify({
      settings: {
        sound: false,
        notifications: false,
        autoStartBreaks: false,
        silenceBannersDuringFocus: false,
        notificationFilter: { enabled: false },
      },
    }),
    { mode: 0o600 },
  );

  console.log("Desktop smoke: creating a task and saving a one-minute interval.");
  await launch();
  await click('button[aria-label="Open settings"]');
  await fill("#focus-minutes", "1");
  await click('.settings-dialog button[type="submit"]');
  await state((data) => data.settings.focusMinutes === 1, "saved duration");
  await click('button[aria-label^="Theme:"]');
  await state((data) => data.settings.theme === "light", "saved theme");

  await command("POST", "/actions", {
    actions: [
      {
        type: "key",
        id: "keyboard",
        actions: [
          { type: "keyDown", value: "\uE009" },
          { type: "keyDown", value: "n" },
          { type: "keyUp", value: "n" },
          { type: "keyUp", value: "\uE009" },
        ],
      },
    ],
  });
  await until("keyboard focus in task composer", () =>
    evaluate('return document.activeElement?.id === "new-task-title";'),
  );
  await fill("#new-task-title", "Desktop smoke task");
  await click('.task-form button[type="submit"]');
  await click(".task-select");
  await state(
    (data) => data.tasks.length === 1 && data.timer.activeTaskId === data.tasks[0].id,
    "task selection",
  );
  await click(".timer-controls .primary-control");
  await state((data) => data.timer.status === "running", "running timer");
  await until("visible countdown", () =>
    evaluate(
      'return /^00:[0-5][0-9]$/.test(document.querySelector(".face-digits")?.textContent?.trim() ?? "");',
    ),
  );
  await click(".timer-controls .primary-control");
  const paused = await state((data) => data.timer.status === "paused", "paused timer");
  await screenshot("paused-focus");
  await stop();

  console.log("Desktop smoke: restarting with paused state, then a running deadline.");
  await launch();
  await until("restored paused UI", () =>
    evaluate('return document.querySelector(".app-shell")?.classList.contains("status-paused");'),
  );
  assert.equal(
    await evaluate(
      'return document.querySelector(".task-select .task-title")?.textContent?.trim();',
    ),
    "Desktop smoke task",
  );
  assert.equal(await evaluate("return document.documentElement.dataset.theme;"), "light");
  assert.equal((await readStore()).timer.remainingSeconds, paused.timer.remainingSeconds);
  await click(".timer-controls .primary-control");
  const running = await state((data) => data.timer.status === "running", "resumed timer");
  await stop();
  await launch();
  assert.equal(
    (await readStore()).timer.endsAt,
    running.timer.endsAt,
    "Restart must preserve the absolute deadline",
  );
  await until("restored running UI", () =>
    evaluate('return document.querySelector(".app-shell")?.classList.contains("status-running");'),
  );

  console.log("Desktop smoke: waiting for real completion and exactly one credited session.");
  const completed = await until(
    "focus completion",
    async () => {
      const data = await readStore();
      return data.sessions.some((record) => record.outcome === "completed") && data;
    },
    75_000,
  );
  assert.equal(completed.sessions.length, 1);
  assert.equal(completed.sessions[0].durationSeconds, 60);
  assert.equal(completed.tasks[0].completedPomodoros, 1);
  assert.equal(completed.timer.phase, "shortBreak");
  await until("completed UI", () =>
    evaluate(
      'return document.querySelector(".app-shell")?.classList.contains("phase-shortBreak");',
    ),
  );
  await screenshot("completed-focus");
  await stop();

  console.log("Desktop smoke: preserving and reporting a corrupted store.");
  const damaged = "{ deliberate smoke-test corruption";
  await writeFile(storePath, damaged, { mode: 0o600 });
  await launch();
  await element('.store-banner[role="alert"]');
  await screenshot("recovered-store");
  const files = await readdir(dirname(storePath));
  const recovered = files.find((name) => name.startsWith("pomodoro.unreadable-"));
  assert.ok(recovered, "Unreadable store should be preserved beside the new store");
  assert.equal(await readFile(join(dirname(storePath), recovered), "utf8"), damaged);
  await stop();
  await rm(temporary, { recursive: true, force: true });
  temporary = undefined;
  console.log(
    "Desktop smoke passed: task, keyboard, settings, pause/resume, restarts, completion, recovery.",
  );
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => {
    void stop().finally(() => process.exit(signal === "SIGINT" ? 130 : 143));
  });
}

try {
  await smoke();
} catch (error) {
  await screenshot("failure").catch(() => {});
  console.error(error);
  if (driverLog) console.error(driverLog);
  if (temporary) console.error(`Isolated test data retained for inspection: ${temporary}`);
  process.exitCode = 1;
} finally {
  await stop();
}
