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

// Mirrors the FENCE regex in app/src/changelog/parse.ts, so an entry detail
// containing a worked markdown example (a fenced block with its own `### `
// line) is not mistaken here for a real pending entry.
const FENCE = /^\s*(```|~~~)/

export function isValidVersion(version) {
  return /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$/.test(version)
}

/**
 * Major, minor and patch decide it first. On a tie, a final version outranks
 * every prerelease that shares its X.Y.Z, so promoting a release candidate to
 * its final version - `isNewerVersion('0.1.24', '0.1.24-rc.1')` - is a step
 * forward rather than a rejection. The reverse is still false: a prerelease
 * is never newer than the matching final version. Two prereleases of the same
 * X.Y.Z compare their suffixes identifier by identifier (split on `.`),
 * comparing numerically when both sides of a pair are digits and lexically
 * otherwise, so `rc.2` beats `rc.10` the way it would under semver precedence
 * rather than losing to it on plain string order.
 */
export function isNewerVersion(candidate, current) {
  const parts = (v) => v.split('-')[0].split('.').map((n) => Number.parseInt(n, 10) || 0)
  const prerelease = (v) => {
    const i = v.indexOf('-')
    return i < 0 ? null : v.slice(i + 1)
  }
  // Compare two prerelease suffixes identifier by identifier. Returns
  // negative/zero/positive the way `Array.prototype.sort` comparators do.
  const compareSuffix = (a, b) => {
    const as = a.split('.')
    const bs = b.split('.')
    const len = Math.max(as.length, bs.length)
    for (let i = 0; i < len; i++) {
      if (as[i] === undefined) return -1
      if (bs[i] === undefined) return 1
      const an = /^\d+$/.test(as[i])
      const bn = /^\d+$/.test(bs[i])
      if (an && bn) {
        const diff = Number.parseInt(as[i], 10) - Number.parseInt(bs[i], 10)
        if (diff !== 0) return diff
      } else if (as[i] !== bs[i]) {
        return as[i] < bs[i] ? -1 : 1
      }
    }
    return 0
  }

  const a = parts(candidate)
  const b = parts(current)
  for (let i = 0; i < 3; i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0)
    if (diff !== 0) return diff > 0
  }

  const pa = prerelease(candidate)
  const pb = prerelease(current)
  if (pa === null && pb === null) return false
  if (pa === null) return true
  if (pb === null) return false
  return compareSuffix(pa, pb) > 0
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
  // Match through the heading's own line terminator rather than stopping at
  // `$`, so the exact bytes that separated "## Unreleased" from the rest of
  // the file (this repo's CHANGELOG.md is CRLF on disk) are captured instead
  // of assumed. Reusing that terminator for both freshly written lines keeps
  // the whole file on one line-ending style - a hardcoded `\n` would leave a
  // silently mixed CRLF/LF file on a CRLF checkout.
  //
  // `## +Unreleased` (one or more spaces), not a literal single space: this
  // has to accept whatever `changelog-check.sh` and `parse.ts` already accept,
  // or a heading with two spaces after `##` passes CI and renders in the app
  // while this function throws as if the section were missing.
  const re = /^## +Unreleased(\r\n|\n)/m
  if (!re.test(markdown)) throw new Error('no "## Unreleased" heading found in CHANGELOG.md')
  return markdown.replace(re, (whole, eol) => `## Unreleased${eol}${eol}## ${version} - ${date}${eol}`)
}

/**
 * The entries currently sitting under `## Unreleased`.
 *
 * Fence-aware like `parse.ts`: a `### ` line inside a fenced markdown example
 * in an entry's detail is content, not a second pending entry.
 */
export function unreleasedEntries(markdown) {
  const start = markdown.search(/^## +Unreleased[^\n]*$/m)
  if (start < 0) return []
  const rest = markdown.slice(start).split('\n').slice(1)
  const out = []
  let inFence = false
  let fenceMarker = ''
  for (const line of rest) {
    const fence = line.match(FENCE)
    if (fence) {
      // Only a matching marker closes the block, so ``` inside a ~~~ block is
      // content rather than a terminator - same rule as parse.ts.
      if (!inFence) {
        inFence = true
        fenceMarker = fence[1]
      } else if (line.trim().startsWith(fenceMarker)) {
        inFence = false
        fenceMarker = ''
      }
      continue
    }
    if (inFence) continue
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
