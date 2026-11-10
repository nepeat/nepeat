#!/usr/bin/env node

/**
 * Claude Code statusline: quota, path, model, context and OpenViking state.
 *
 * Installed to ~/.claude/statusline.mjs by module/claude.nix, which also
 * points ~/.claude/settings.json `.statusLine` at it. Claude Code runs this
 * as a fresh process on each conversation update with a JSON payload on
 * stdin and renders whatever lands on stdout.
 *
 * The OpenViking segments are a self-contained port of
 * examples/claude-code-memory-plugin/scripts/statusline.mjs from the
 * OpenViking repo: same state files, same /health probe cache, same glyphs.
 * It is standalone (no plugin imports) so it works whether or not the plugin
 * is installed. Not ported: the plugin's per-session bypass patterns.
 *
 * Layout:
 *   ~/path │ Model · high · ctx 58% left │ 5h 80% · 2h13m │ wk 64% │ OV ✓ │ ↩ 3 mem …
 *
 * Empty stdout is a valid statusline, and this must never crash Claude Code.
 */

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const HOME = homedir();
const STATE_MAX_AGE_MS = 30 * 60_000; // 30 min: older recall/capture = idle
const SESSION_EVENT_MAX_AGE_MS = 60_000; // 1 min: "🔗 resumed" fades
const PROBE_TTL_MS = 5000;
const PROBE_TIMEOUT_MS = 1000;
const MAX_WIDTH = Number(process.env.STATUSLINE_MAX_WIDTH) || 160;
const ESC = "\x1b[";

const COLOR = !process.env.NO_COLOR
  && !process.env.OPENVIKING_STATUSLINE_NO_COLOR
  && process.env.TERM !== "dumb";
const c = (code, s) => (COLOR ? `${ESC}${code}m${s}${ESC}0m` : s);
const dim = (s) => c("2", s);
const green = (s) => c("32", s);
const red = (s) => c("31", s);
const yellow = (s) => c("33", s);
const cyan = (s) => c("36", s);
const blue = (s) => c("34", s);

const num = (v) => (typeof v === "number" && Number.isFinite(v) ? v : null);
const clampPct = (v) => Math.min(100, Math.max(0, Math.round(v)));

function human(n) {
  if (typeof n !== "number" || !Number.isFinite(n)) return "?";
  if (n < 1000) return String(n);
  if (n < 10_000) return (n / 1000).toFixed(1) + "k";
  return Math.round(n / 1000) + "k";
}

// Colour a "percent remaining" figure: plenty = dim, low = yellow, nearly out = red.
function byRemaining(remaining, text) {
  if (remaining <= 10) return red(text);
  if (remaining <= 30) return yellow(text);
  return dim(text);
}

// ---------------------------------------------------------------- stdin

async function readStdin() {
  if (process.stdin.isTTY) return { raw: "", data: {} };
  return await new Promise((resolve) => {
    const chunks = [];
    let settled = false;
    const settle = () => {
      if (settled) return;
      settled = true;
      const raw = Buffer.concat(chunks).toString();
      let data = {};
      try {
        data = JSON.parse(raw || "{}") || {};
      } catch { /* malformed payload: render what we can */ }
      resolve({ raw, data });
    };
    // Claude Code writes stdin promptly; never block the render waiting on it.
    const timer = setTimeout(settle, 250);
    process.stdin.on("data", (chunk) => chunks.push(chunk));
    process.stdin.on("end", () => { clearTimeout(timer); settle(); });
    process.stdin.on("error", () => { clearTimeout(timer); settle(); });
  });
}

// Orca installs its own statusLine command to collect rate-limit data for its
// UI. A statusLine setting holds one command, so this script takes the slot
// and hands the same payload on to Orca's hook, detached, when it exists.
function forwardToOrca(raw) {
  if (!raw) return;
  const hook = join(HOME, ".orca", "agent-hooks", "claude-statusline.sh");
  if (!existsSync(hook)) return;
  try {
    const child = spawn("/bin/sh", [hook], {
      detached: true,
      stdio: ["pipe", "ignore", "ignore"],
    });
    child.on("error", () => {});
    child.stdin.on("error", () => {});
    child.stdin.end(raw);
    child.unref();
  } catch { /* best effort */ }
}

// ------------------------------------------------- Claude Code segments

function pathSegment(stdin) {
  const dir = stdin.workspace?.current_dir || stdin.cwd;
  if (typeof dir !== "string" || !dir) return null;
  const short = dir === HOME ? "~" : dir.startsWith(HOME + "/") ? "~" + dir.slice(HOME.length) : dir;
  return blue(short);
}

function contextRemaining(stdin) {
  const cw = stdin.context_window;
  if (!cw || typeof cw !== "object") return null;
  const remaining = num(cw.remaining_percentage);
  if (remaining !== null) return clampPct(remaining);
  const usedPct = num(cw.used_percentage);
  if (usedPct !== null) return clampPct(100 - usedPct);
  const used = num(cw.total_input_tokens);
  const size = num(cw.context_window_size);
  if (used !== null && size !== null && size > 0) return clampPct(100 - (used / size) * 100);
  return null;
}

// Thinking level: `effort.level` (low … max) when thinking is on, "think off"
// when `thinking.enabled` is false. Skipped if the payload carries neither.
function thinkingLabel(stdin) {
  if (stdin.thinking?.enabled === false) return "think off";
  const level = typeof stdin.effort?.level === "string" ? stdin.effort.level.trim() : "";
  return level || null;
}

// One cluster: "Mythos 5.2 · high · ctx 58% left".
function modelCtxSegment(stdin) {
  const name = typeof stdin.model?.display_name === "string" ? stdin.model.display_name.trim() : "";
  const think = thinkingLabel(stdin);
  const remaining = contextRemaining(stdin);
  const bits = [
    name ? cyan(name) : null,
    think ? c("35", think) : null,
    remaining === null ? null : byRemaining(remaining, `ctx ${remaining}% left`),
  ].filter(Boolean);
  return bits.length ? bits.join(dim(" · ")) : null;
}

// resets_at arrives as epoch seconds; accept ms or an ISO string too.
function resetCountdown(resetsAt) {
  let ms = null;
  if (typeof resetsAt === "number" && Number.isFinite(resetsAt)) {
    ms = resetsAt < 1e12 ? resetsAt * 1000 : resetsAt;
  } else if (typeof resetsAt === "string") {
    const parsed = Date.parse(resetsAt);
    if (Number.isFinite(parsed)) ms = parsed;
  }
  if (ms === null) return null;
  const mins = Math.round((ms - Date.now()) / 60_000);
  if (mins <= 0) return null;
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h${String(mins % 60).padStart(2, "0")}m`;
  return `${Math.floor(hours / 24)}d${hours % 24}h`;
}

// Quota remaining for one rate-limit window. Absent (API-key sessions, or
// before the first response of a session) means the segment is skipped.
function quotaSegment(label, window) {
  const used = num(window?.used_percentage);
  if (used === null) return null;
  const remaining = clampPct(100 - used);
  const reset = resetCountdown(window.resets_at);
  return byRemaining(remaining, `${label} ${remaining}%${reset ? ` · ${reset}` : ""}`);
}

// --------------------------------------------------- OpenViking segments

const expandHome = (p) => p.replace(/^~(?=$|\/)/, HOME);
const OV_HOME = process.env.OPENVIKING_HOME?.trim()
  ? expandHome(process.env.OPENVIKING_HOME.trim())
  : join(HOME, ".openviking");
const STATE_DIR = join(OV_HOME, "state");

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf-8"));
  } catch {
    return null;
  }
}

function readJsonState(name, { maxAgeMs } = {}) {
  const parsed = readJson(join(STATE_DIR, name));
  if (!parsed) return null;
  if (maxAgeMs && typeof parsed.ts === "number" && Date.now() - parsed.ts > maxAgeMs) return null;
  return parsed;
}

function envBool(name) {
  const v = (process.env[name] || "").trim().toLowerCase();
  if (v === "0" || v === "false" || v === "no") return false;
  if (v === "1" || v === "true" || v === "yes") return true;
  return undefined;
}

// Connection info, same precedence as the plugin: env, then ovcli.conf, then
// ov.conf's server section. Returns null when OpenViking is not set up here.
function loadOvConfig() {
  const enabled = envBool("OPENVIKING_MEMORY_ENABLED");
  if (enabled === false) return null;

  const cli = readJson(expandHome(process.env.OPENVIKING_CLI_CONFIG_FILE || join(OV_HOME, "ovcli.conf")));
  const ov = readJson(expandHome(process.env.OPENVIKING_CONFIG_FILE || join(OV_HOME, "ov.conf")));
  if (ov?.claude_code?.enabled === false && enabled !== true) return null;

  const server = ov?.server || {};
  let baseUrl = process.env.OPENVIKING_URL || process.env.OPENVIKING_BASE_URL || cli?.url || server.url;
  if (!baseUrl && server.host && server.port) {
    const host = server.host === "0.0.0.0" ? "127.0.0.1" : server.host;
    baseUrl = `http://${host}:${server.port}`;
  }
  if (!baseUrl) return null;

  return {
    baseUrl: String(baseUrl).replace(/\/+$/, ""),
    apiKey: process.env.OPENVIKING_API_KEY || process.env.OPENVIKING_BEARER_TOKEN
      || cli?.api_key || server.root_api_key || "",
  };
}

// GET /health, cached for 5 s in a file shared by every session on the machine
// so N open tabs do not mean N probes per turn. Always resolves.
async function probeServer(cfg) {
  const cacheFile = join(STATE_DIR, "server-probe.json");
  const cached = readJson(cacheFile);
  if (cached && typeof cached.ts === "number" && Date.now() - cached.ts < PROBE_TTL_MS
    && cached.base_url === cfg.baseUrl) {
    return cached;
  }

  const headers = { "Content-Type": "application/json" };
  if (cfg.apiKey) headers.Authorization = `Bearer ${cfg.apiKey}`;

  const t0 = Date.now();
  let healthy = false;
  let error;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    const res = await fetch(`${cfg.baseUrl}/health`, { headers, signal: controller.signal });
    healthy = res.ok;
    if (!healthy) error = `health_${res.status}`;
  } catch (err) {
    error = err?.name === "AbortError" ? "timeout" : (err?.message || "unreachable");
  } finally {
    clearTimeout(timer);
  }

  const payload = { healthy, latency_ms: Date.now() - t0, base_url: cfg.baseUrl, error, ts: Date.now() };
  try {
    mkdirSync(STATE_DIR, { recursive: true });
    writeFileSync(cacheFile, JSON.stringify(payload));
  } catch { /* best effort */ }
  return payload;
}

async function ovSegments(stdin) {
  if (process.env.OPENVIKING_STATUSLINE === "off") return [];
  const cfg = loadOvConfig();
  if (!cfg) return [dim("OV –")];

  const sessionId = stdin.session_id;
  const parts = [];

  const probe = await probeServer(cfg);
  if (probe.healthy) {
    parts.push(green("OV ✓"));
  } else if (probe.error === "timeout") {
    // Missed the 1 s budget: possibly alive but lagging. Red is kept for
    // actual unreachability.
    parts.push(yellow("OV ⚠ slow"));
  } else {
    parts.push(red("OV ✗ offline"));
  }

  // Recall: only when memories were actually injected this turn.
  const recall = readJsonState("last-recall.json", { maxAgeMs: STATE_MAX_AGE_MS });
  if (recall && recall.reason === "ok" && recall.count > 0) {
    parts.push(dim(`↩ ${recall.count} mem`
      + (typeof recall.latency_ms === "number" ? ` · ${recall.latency_ms}ms` : "")));
  }

  // Capture: pending tokens toward the next archive, plus archives so far.
  const capture = readJsonState("last-capture.json", { maxAgeMs: STATE_MAX_AGE_MS });
  if (capture && capture.cc_session_id === sessionId) {
    const archived = Number(capture.commit_count || 0);
    const archivedTail = archived > 0 ? ` · ${archived} arch` : "";
    if (capture.committed) {
      parts.push(dim(`✎ committed${archivedTail}`));
    } else if (capture.pending_tokens > 0) {
      parts.push(dim(`✎ ${human(capture.pending_tokens)}/${human(capture.commit_threshold)}${archivedTail}`));
    } else if (archived > 0) {
      parts.push(dim(`✎ ${archived} arch`));
    }
    if (Number(capture.turns_failed) > 0) {
      parts.push(red(`✗ ${capture.turns_failed} dropped`));
    }
  }

  // Shown for a minute after SessionStart re-hydrated context.
  const sessionEvent = readJsonState("last-session-event.json", { maxAgeMs: SESSION_EVENT_MAX_AGE_MS });
  if (sessionEvent && sessionEvent.cc_session_id === sessionId) {
    parts.push(cyan(`🔗 ${sessionEvent.source === "compact" ? "compact" : "resumed"}`));
  }

  // Archives across all sessions today.
  const daily = readJsonState("daily-stats.json");
  if (daily && daily.date === new Date().toISOString().slice(0, 10) && Number(daily.archives) > 0) {
    parts.push(dim(`+${daily.archives} today`));
  }

  return parts;
}

// ---------------------------------------------------------------- render

function truncate(line) {
  // eslint-disable-next-line no-control-regex
  const visible = line.replace(/\x1b\[[0-9;]*m/g, "");
  if (visible.length <= MAX_WIDTH) return line;
  let out = "";
  let visibleLen = 0;
  let i = 0;
  while (i < line.length && visibleLen < MAX_WIDTH - 1) {
    if (line[i] === "\x1b") {
      // eslint-disable-next-line no-control-regex
      const m = line.slice(i).match(/^\x1b\[[0-9;]*m/);
      if (m) { out += m[0]; i += m[0].length; continue; }
    }
    out += line[i];
    visibleLen++;
    i++;
  }
  return out + "…" + (COLOR ? `${ESC}0m` : "");
}

async function main() {
  const { raw, data: stdin } = await readStdin();
  forwardToOrca(raw);

  const parts = [
    pathSegment(stdin),
    modelCtxSegment(stdin),
    quotaSegment("5h", stdin.rate_limits?.five_hour),
    quotaSegment("wk", stdin.rate_limits?.seven_day),
    // OpenViking goes last so it is what gets cut when the terminal narrows.
    ...(await ovSegments(stdin).catch(() => [])),
  ].filter(Boolean);

  process.stdout.write(truncate(parts.join(dim(" │ "))));
}

main().catch(() => { /* statusline must never crash Claude Code */ });
