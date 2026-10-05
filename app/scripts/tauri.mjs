import { spawn } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';

function normalizeFeaturesArg(existingValue) {
  const parts = String(existingValue || '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);

  if (!parts.includes('local-stt')) {
    parts.push('local-stt');
  }

  return parts.join(',');
}

function ensureLocalSttFeature(args) {
  const rawArgs = args.length > 0 && args[0] === '--' ? args.slice(1) : args;

  const separatorIndex = rawArgs.indexOf('--');
  const tauriArgs = separatorIndex >= 0 ? rawArgs.slice(0, separatorIndex) : [...rawArgs];
  const passthroughArgs = separatorIndex >= 0 ? rawArgs.slice(separatorIndex) : [];

  const subcommand = tauriArgs[0];
  if (subcommand !== 'dev' && subcommand !== 'build') {
    return rawArgs;
  }

  const featuresIndex = tauriArgs.indexOf('--features');
  if (featuresIndex >= 0) {
    const valueIndex = featuresIndex + 1;
    const currentValue = tauriArgs[valueIndex] ?? '';
    tauriArgs[valueIndex] = normalizeFeaturesArg(currentValue);
    return [...tauriArgs, ...passthroughArgs];
  }

  return [...tauriArgs, '--features', 'local-stt', ...passthroughArgs];
}

/**
 * Keep a Rust build from taking the machine over.
 *
 * Two levers, both overridable:
 * - Priority. This process drops to below normal before it spawns Tauri. On
 *   Windows a child of a below-normal process inherits that class, so cargo,
 *   every rustc and the linker run below normal too and the desktop stays
 *   responsive. AIDC_BUILD_PRIORITY=normal turns it off. In `dev` the app
 *   itself is a child as well and runs below normal for that session.
 * - Parallelism. Unless CARGO_BUILD_JOBS is already set, cargo gets all cores
 *   but two. AIDC_BUILD_JOBS=<n> picks a number; 0 means cargo's default.
 */
function limitBuildLoad(env) {
  if ((process.env.AIDC_BUILD_PRIORITY || '').toLowerCase() !== 'normal') {
    try {
      os.setPriority(os.constants.priority.PRIORITY_BELOW_NORMAL);
    } catch (e) {
      console.warn(`[tauri wrapper] could not lower build priority: ${e?.message || e}`);
    }
  }
  if (!env.CARGO_BUILD_JOBS) {
    const requested = process.env.AIDC_BUILD_JOBS;
    const jobs = requested !== undefined && requested !== ''
      ? Number(requested)
      : Math.max(1, os.availableParallelism() - 2);
    if (Number.isFinite(jobs) && jobs > 0) env.CARGO_BUILD_JOBS = String(Math.floor(jobs));
  }
  return env;
}

const args = ensureLocalSttFeature(process.argv.slice(2));
const sub = args.find((a) => !a.startsWith('-'));
const env = sub === 'dev' || sub === 'build' ? limitBuildLoad({ ...process.env }) : process.env;
if (env.CARGO_BUILD_JOBS && env !== process.env) {
  console.log(`[tauri wrapper] cargo jobs: ${env.CARGO_BUILD_JOBS}, priority: ${(process.env.AIDC_BUILD_PRIORITY || 'below normal')}`);
}
const tauriBin = process.platform === 'win32'
  ? path.join('node_modules', '.bin', 'tauri.cmd')
  : path.join('node_modules', '.bin', 'tauri');

const child = spawn(tauriBin, args, {
  stdio: 'inherit',
  shell: process.platform === 'win32',
  env,
});

child.on('exit', (code) => {
  process.exit(code ?? 0);
});
