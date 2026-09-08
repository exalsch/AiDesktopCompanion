import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  rollChangelog,
  bumpJsonVersion,
  bumpPackageLockVersion,
  bumpCargoTomlVersion,
  bumpCargoLockVersion,
  isValidVersion,
  isNewerVersion,
  unreleasedEntries,
} from '../../scripts/release-prep.mjs'

test('isValidVersion accepts semver with an optional prerelease suffix', () => {
  assert.equal(isValidVersion('0.1.24'), true)
  assert.equal(isValidVersion('1.0.0-rc.1'), true)
  assert.equal(isValidVersion('v0.1.24'), false)
  assert.equal(isValidVersion('0.1'), false)
  assert.equal(isValidVersion('not-a-version'), false)
})

test('isNewerVersion refuses anything that is not a step forward', () => {
  assert.equal(isNewerVersion('0.1.24', '0.1.23'), true)
  assert.equal(isNewerVersion('0.2.0', '0.1.99'), true)
  assert.equal(isNewerVersion('1.0.0', '0.9.9'), true)
  assert.equal(isNewerVersion('0.1.23', '0.1.23'), false)
  assert.equal(isNewerVersion('0.1.22', '0.1.23'), false)
})

test('isNewerVersion lets a final version promote its own prerelease', () => {
  assert.equal(isNewerVersion('0.1.24', '0.1.24-rc.1'), true)
})

test('isNewerVersion still refuses a prerelease against the matching final version', () => {
  assert.equal(isNewerVersion('0.1.24-rc.1', '0.1.24'), false)
})

test('isNewerVersion compares two prereleases of the same version by suffix', () => {
  assert.equal(isNewerVersion('0.1.24-rc.2', '0.1.24-rc.1'), true)
  assert.equal(isNewerVersion('0.1.24-rc.1', '0.1.24-rc.2'), false)
  assert.equal(isNewerVersion('0.1.24-rc.1', '0.1.24-rc.1'), false)
  // Numeric comparison per identifier, not string comparison of the whole
  // suffix: 'rc.10' must beat 'rc.9' even though '1' sorts before '9'.
  assert.equal(isNewerVersion('0.1.24-rc.10', '0.1.24-rc.9'), true)
})

test('unreleasedEntries lists the pending titles and stops at the next version', () => {
  const md = '## Unreleased\n\n### One\n\nDetail.\n\n### Two\n\n## 0.1.23 - 2026-09-07\n\n### Already released\n'
  assert.deepEqual(unreleasedEntries(md), ['One', 'Two'])
  assert.deepEqual(unreleasedEntries('## 0.1.23 - 2026-09-07\n\n### Released\n'), [])
})

test('unreleasedEntries recognises "##  Unreleased" with extra spaces', () => {
  const md = '##  Unreleased\n\n### One\n\n## 0.1.23 - 2026-09-07\n\n### Already released\n'
  assert.deepEqual(unreleasedEntries(md), ['One'])
})

test('unreleasedEntries does not count a ### line inside a fenced example', () => {
  const md = [
    '## Unreleased',
    '',
    '### A real entry',
    '',
    'kind: feat',
    '',
    'Example:',
    '',
    '```md',
    '### Not a real entry',
    '```',
    '',
    '### Another real entry',
    '',
    '## 0.1.23 - 2026-09-07',
    '',
    '### Already released',
    '',
  ].join('\n')
  assert.deepEqual(unreleasedEntries(md), ['A real entry', 'Another real entry'])
})

test('rollChangelog dates the Unreleased section and opens a fresh one', () => {
  const before = `# Changelog

Intro.

## Unreleased

### A change

kind: feat

Detail.

## 0.1.23 - 2026-09-07

### Older.
`
  const after = rollChangelog(before, '0.1.24', '2026-09-08')
  assert.match(after, /## Unreleased\n\n## 0\.1\.24 - 2026-09-08\n/)
  assert.match(after, /## 0\.1\.24 - 2026-09-08\n\n### A change/)
  assert.match(after, /## 0\.1\.23 - 2026-09-07/)
  // Exactly one Unreleased section survives, and it is empty.
  assert.equal(after.match(/^## Unreleased$/gm)?.length, 1)
})

test('rollChangelog throws when there is no Unreleased section', () => {
  assert.throws(() => rollChangelog('# Changelog\n\n## 0.1.23 - 2026-09-07\n', '0.1.24', '2026-09-08'), /Unreleased/)
})

test('rollChangelog recognises "##  Unreleased" with extra spaces', () => {
  const before = '# Changelog\n\n##  Unreleased\n\n### A change\n\nDetail.\n\n## 0.1.23 - 2026-09-07\n\n### Older.\n'
  const after = rollChangelog(before, '0.1.24', '2026-09-08')
  assert.match(after, /## Unreleased\n\n## 0\.1\.24 - 2026-09-08\n/)
  assert.match(after, /## 0\.1\.24 - 2026-09-08\n\n### A change/)
  assert.equal(after.match(/^## Unreleased$/gm)?.length, 1)
})

test('rollChangelog preserves CRLF line endings on a CRLF source file', () => {
  // CHANGELOG.md is CRLF on a typical Windows checkout of this repo; feed the
  // real byte pattern rather than a `\n`-only fixture, since a loosened
  // assertion regex on an LF fixture would pass whether or not the CRLF case
  // actually works.
  const before = [
    '# Changelog',
    '',
    'Intro.',
    '',
    '## Unreleased',
    '',
    '### A change',
    '',
    'kind: feat',
    '',
    'Detail.',
    '',
    '## 0.1.23 - 2026-09-07',
    '',
    '### Older.',
    '',
  ].join('\r\n')
  const after = rollChangelog(before, '0.1.24', '2026-09-08')
  assert.match(after, /## Unreleased\r\n\r\n## 0\.1\.24 - 2026-09-08\r\n/)
  assert.match(after, /## 0\.1\.24 - 2026-09-08\r\n\r\n### A change/)
  assert.match(after, /## 0\.1\.23 - 2026-09-07/)
  assert.equal(after.match(/^## Unreleased$/gm)?.length, 1)
  // No bare LF anywhere in the result: a CRLF file must stay CRLF throughout,
  // not just around the two lines this function writes.
  assert.equal(/[^\r]\n/.test(after), false)
})

test('bumpJsonVersion replaces only the top-level version field', () => {
  const before = '{\n  "name": "AiDesktopCompanion",\n  "version": "0.1.23",\n  "dependencies": {\n    "vue": "^3.5.41"\n  }\n}\n'
  const after = bumpJsonVersion(before, '0.1.24')
  assert.match(after, /"version": "0\.1\.24"/)
  assert.match(after, /"vue": "\^3\.5\.41"/)
})

test('bumpPackageLockVersion updates both the root and the self entry', () => {
  const before = '{\n  "name": "AiDesktopCompanion",\n  "version": "0.1.23",\n  "packages": {\n    "": {\n      "name": "AiDesktopCompanion",\n      "version": "0.1.23"\n    },\n    "node_modules/vue": {\n      "version": "3.5.41"\n    }\n  }\n}\n'
  const after = bumpPackageLockVersion(before, '0.1.24')
  assert.equal(after.match(/"version": "0\.1\.24"/g)?.length, 2)
  assert.match(after, /"version": "3\.5\.41"/)
})

test('bumpCargoTomlVersion changes the package version, not a dependency version', () => {
  const before = '[package]\nname = "AiDesktopCompanion"\nversion = "0.1.23"\nedition = "2021"\n\n[dependencies]\nserde = { version = "1.0" }\n'
  const after = bumpCargoTomlVersion(before, '0.1.24')
  assert.match(after, /\[package\]\nname = "AiDesktopCompanion"\nversion = "0\.1\.24"/)
  assert.match(after, /serde = \{ version = "1\.0" \}/)
})

test('bumpCargoLockVersion changes only the named package block', () => {
  const before = '[[package]]\nname = "AiDesktopCompanion"\nversion = "0.1.23"\ndependencies = [\n "arboard",\n]\n\n[[package]]\nname = "arboard"\nversion = "0.1.23"\n'
  const after = bumpCargoLockVersion(before, 'AiDesktopCompanion', '0.1.24')
  assert.match(after, /name = "AiDesktopCompanion"[\r\n]+version = "0\.1\.24"/)
  assert.match(after, /name = "arboard"[\r\n]+version = "0\.1\.23"/)
})

test('bumpJsonVersion works on CRLF input', () => {
  const before = '{\r\n  "name": "AiDesktopCompanion",\r\n  "version": "0.1.23",\r\n  "dependencies": {\r\n    "vue": "^3.5.41"\r\n  }\r\n}\r\n'
  const after = bumpJsonVersion(before, '0.1.24')
  assert.match(after, /"version": "0\.1\.24"/)
  assert.match(after, /"vue": "\^3\.5\.41"/)
})

test('bumpPackageLockVersion works on CRLF input', () => {
  const before = '{\r\n  "name": "AiDesktopCompanion",\r\n  "version": "0.1.23",\r\n  "packages": {\r\n    "": {\r\n      "name": "AiDesktopCompanion",\r\n      "version": "0.1.23"\r\n    },\r\n    "node_modules/vue": {\r\n      "version": "3.5.41"\r\n    }\r\n  }\r\n}\r\n'
  const after = bumpPackageLockVersion(before, '0.1.24')
  assert.equal(after.match(/"version": "0\.1\.24"/g)?.length, 2)
  assert.match(after, /"version": "3\.5\.41"/)
})

test('bumpCargoTomlVersion works on CRLF input', () => {
  const before = '[package]\r\nname = "AiDesktopCompanion"\r\nversion = "0.1.23"\r\nedition = "2021"\r\n\r\n[dependencies]\r\nserde = { version = "1.0" }\r\n'
  const after = bumpCargoTomlVersion(before, '0.1.24')
  assert.match(after, /\[package\]\r\nname = "AiDesktopCompanion"\r\nversion = "0\.1\.24"/)
  assert.match(after, /serde = \{ version = "1\.0" \}/)
})

test('bumpCargoLockVersion works on CRLF input', () => {
  const before = '[[package]]\r\nname = "AiDesktopCompanion"\r\nversion = "0.1.23"\r\ndependencies = [\r\n "arboard",\r\n]\r\n\r\n[[package]]\r\nname = "arboard"\r\nversion = "0.1.23"\r\n'
  const after = bumpCargoLockVersion(before, 'AiDesktopCompanion', '0.1.24')
  assert.match(after, /name = "AiDesktopCompanion"\r\nversion = "0\.1\.24"/)
  assert.match(after, /name = "arboard"\r\nversion = "0\.1\.23"/)
})

test('unreleasedEntries works on CRLF input', () => {
  const md = '## Unreleased\r\n\r\n### One\r\n\r\nDetail.\r\n\r\n### Two\r\n\r\n## 0.1.23 - 2026-09-07\r\n\r\n### Already released\r\n'
  assert.deepEqual(unreleasedEntries(md), ['One', 'Two'])
})

test('each bump helper throws when its target is missing', () => {
  assert.throws(() => bumpJsonVersion('{}', '0.1.24'), /version/)
  assert.throws(() => bumpCargoTomlVersion('[dependencies]\n', '0.1.24'), /\[package\]/)
  assert.throws(() => bumpCargoLockVersion('[[package]]\nname = "other"\nversion = "1.0.0"\n', 'AiDesktopCompanion', '0.1.24'), /AiDesktopCompanion/)
})
