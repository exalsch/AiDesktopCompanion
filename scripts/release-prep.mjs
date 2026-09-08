#!/usr/bin/env node
/**
 * Cut a version: bump the five files that carry it, and file the pending
 * changelog entries under it.
 *
 * Usage: node scripts/release-prep.mjs 0.1.24 [--allow-empty]
 *
 * Deliberately does not commit, tag or push. It writes files and prints what it
 * touched; the release itself stays a human decision.
 *
 * Every edit below insists on exactly one match and throws otherwise. A loud
 * failure that tells you to edit by hand beats a silent one that ships a
 * half-bumped tree.
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const CARGO_PACKAGE = 'AiDesktopCompanion'

export function isValidVersion(version) {
  return /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$/.test(version)
}

/**
 * Major, minor and patch only. A prerelease suffix does not make a version
 * newer here, so cutting 0.1.24-rc.1 after 0.1.24 is rejected rather than
 * quietly writing a lower release.
 */
export function isNewerVersion(candidate, current) {
  const parts = (v) => v.split('-')[0].split('.').map((n) => Number.parseInt(n, 10) || 0)
  const a = parts(candidate)
  const b = parts(current)
  for (let i = 0; i < 3; i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0)
    if (diff !== 0) return diff > 0
  }
  return false
}

/** Replace the first top-level `"version": "..."`. */
export function bumpJsonVersion(text, version) {
  const re = /^(\s*"version":\s*")[^"]+(")/m
  if (!re.test(text)) throw new Error('no top-level "version" field found')
  return text.replace(re, `$1${version}$2`)
}

/**
 * `package-lock.json` states the version twice: at the root and in
 * `packages[""]`. Both have to move or npm rewrites the file on the next
 * install.
 */
export function bumpPackageLockVersion(text, version) {
  const re = /^(\s*"version":\s*")[^"]+(")/gm
  const matches = text.match(re)
  if (!matches || matches.length < 2) {
    throw new Error('expected at least two "version" fields in the lockfile')
  }
  let seen = 0
  return text.replace(re, (whole, head, tail) => (seen++ < 2 ? `${head}${version}${tail}` : whole))
}

/** The `version` inside `[package]`, never one inside `[dependencies]`. */
export function bumpCargoTomlVersion(text, version) {
  const re = /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+(")/
  if (!re.test(text)) throw new Error('no version under [package] found')
  return text.replace(re, `$1${version}$2`)
}

/** The `version` in the `[[package]]` block whose `name` is `packageName`. */
export function bumpCargoLockVersion(text, packageName, version) {
  const re = new RegExp(`(name = "${packageName}"[\\r\\n]+version = ")[^"]+(")`)
  if (!re.test(text)) throw new Error(`no [[package]] block named ${packageName} found`)
  return text.replace(re, `$1${version}$2`)
}

/**
 * Date the `## Unreleased` heading and open a fresh empty one above it.
 *
 * Entries move with the heading rather than being copied, so a rerun cannot
 * duplicate them.
 */
export function rollChangelog(markdown, version, date) {
  const re = /^## Unreleased[^\n]*$/m
  if (!re.test(markdown)) throw new Error('no "## Unreleased" heading found in CHANGELOG.md')
  return markdown.replace(re, `## Unreleased\n\n## ${version} - ${date}`)
}

/** The entries currently sitting under `## Unreleased`. */
export function unreleasedEntries(markdown) {
  const start = markdown.search(/^## Unreleased[^\n]*$/m)
  if (start < 0) return []
  const rest = markdown.slice(start).split('\n').slice(1)
  const out = []
  for (const line of rest) {
    if (/^## /.test(line)) break
    const m = line.match(/^###\s+(.+?)\s*$/)
    if (m) out.push(m[1])
  }
  return out
}

function today() {
  const d = new Date()
  const pad = (n) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

function main(argv) {
  const args = argv.filter((a) => a !== '--allow-empty')
  const allowEmpty = argv.includes('--allow-empty')
  const version = args[0]

  if (!version || !isValidVersion(version)) {
    console.error('Usage: node scripts/release-prep.mjs <X.Y.Z> [--allow-empty]')
    process.exit(1)
  }

  const files = {
    changelog: join(ROOT, 'CHANGELOG.md'),
    pkg: join(ROOT, 'app/package.json'),
    lock: join(ROOT, 'app/package-lock.json'),
    cargoToml: join(ROOT, 'app/src-tauri/Cargo.toml'),
    cargoLock: join(ROOT, 'app/src-tauri/Cargo.lock'),
    tauriConf: join(ROOT, 'app/src-tauri/tauri.conf.json'),
  }

  const read = Object.fromEntries(
    Object.entries(files).map(([k, p]) => [k, readFileSync(p, 'utf8')]),
  )

  const current = JSON.parse(read.pkg).version
  if (!isNewerVersion(version, current)) {
    console.error(`app/package.json is at ${current}; ${version} is not newer. Nothing to do.`)
    process.exit(1)
  }

  const pending = unreleasedEntries(read.changelog)
  if (pending.length === 0 && !allowEmpty) {
    console.error(
      'CHANGELOG.md has no entries under "## Unreleased".\n' +
        'Add one, or pass --allow-empty if this release genuinely changes nothing a user sees.',
    )
    process.exit(1)
  }

  const written = {
    changelog: rollChangelog(read.changelog, version, today()),
    pkg: bumpJsonVersion(read.pkg, version),
    lock: bumpPackageLockVersion(read.lock, version),
    cargoToml: bumpCargoTomlVersion(read.cargoToml, version),
    cargoLock: bumpCargoLockVersion(read.cargoLock, CARGO_PACKAGE, version),
    tauriConf: bumpJsonVersion(read.tauriConf, version),
  }

  for (const [key, path] of Object.entries(files)) {
    writeFileSync(path, written[key])
    console.log(`updated ${path.slice(ROOT.length + 1)}`)
  }

  console.log(`\n${current} -> ${version}, ${pending.length} changelog entr${pending.length === 1 ? 'y' : 'ies'} filed.`)
  console.log('Review the diff, then commit as: chore(release): bump version to ' + version)
}

// Run only when invoked directly, so the test file can import the helpers.
// A path comparison rather than `import.meta.main`, which needs a newer Node
// than the rest of this script does.
const invokedDirectly = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]
if (invokedDirectly) {
  try {
    main(process.argv.slice(2))
  } catch (err) {
    console.error(String(err?.message ?? err))
    process.exit(1)
  }
}
