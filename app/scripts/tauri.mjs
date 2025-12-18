import { spawn } from 'node:child_process';
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

const args = ensureLocalSttFeature(process.argv.slice(2));
const tauriBin = process.platform === 'win32'
  ? path.join('node_modules', '.bin', 'tauri.cmd')
  : path.join('node_modules', '.bin', 'tauri');

const child = spawn(tauriBin, args, {
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

child.on('exit', (code) => {
  process.exit(code ?? 0);
});
