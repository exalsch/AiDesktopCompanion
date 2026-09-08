import { test } from 'node:test'
import assert from 'node:assert/strict'

import { parseChangelog, compareVersions, releasesSince } from '../src/changelog/parse.ts'

const SAMPLE = `# Changelog

Intro prose that belongs to no release.

## Unreleased

### A pending change

kind: feat

Detail for the pending change.

## 1.2.3 - 2026-01-31

### A released change

kind: fix

First paragraph.

Second paragraph.

### An entry with no detail

### An entry with an unknown kind

kind: chore

Body text.

### An entry holding a code fence

\`\`\`md
### This is not a heading
## Neither is this
\`\`\`

Text after the fence.

## Not a version heading

### This entry must be dropped

## 1.2.2

### Older change

Detail.
`

test('parses releases newest first and keeps the Unreleased section', () => {
  const releases = parseChangelog(SAMPLE)
  assert.deepEqual(releases.map(r => r.version), ['Unreleased', '1.2.3', '1.2.2'])
  assert.equal(releases[0].date, undefined)
  assert.equal(releases[1].date, '2026-01-31')
  assert.equal(releases[2].date, undefined)
})

test('reads the title, the kind chip and the detail of an entry', () => {
  const [unreleased] = parseChangelog(SAMPLE)
  assert.equal(unreleased.entries.length, 1)
  assert.equal(unreleased.entries[0].title, 'A pending change')
  assert.equal(unreleased.entries[0].kind, 'feat')
  assert.equal(unreleased.entries[0].detail, 'Detail for the pending change.')
})

test('keeps every paragraph of a multi-paragraph detail', () => {
  const release = parseChangelog(SAMPLE)[1]
  assert.equal(release.entries[0].detail, 'First paragraph.\n\nSecond paragraph.')
})

test('an entry with no body has an empty detail', () => {
  const release = parseChangelog(SAMPLE)[1]
  const entry = release.entries.find(e => e.title === 'An entry with no detail')
  assert.ok(entry)
  assert.equal(entry.detail, '')
  assert.equal(entry.kind, undefined)
})

test('an unrecognised kind is stripped from the detail but sets no chip', () => {
  const release = parseChangelog(SAMPLE)[1]
  const entry = release.entries.find(e => e.title === 'An entry with an unknown kind')
  assert.ok(entry)
  assert.equal(entry.kind, undefined)
  assert.equal(entry.detail, 'Body text.')
})

test('headings inside a fenced code block are not treated as headings', () => {
  const release = parseChangelog(SAMPLE)[1]
  const titles = release.entries.map(e => e.title)
  assert.ok(!titles.includes('This is not a heading'))
  const entry = release.entries.find(e => e.title === 'An entry holding a code fence')
  assert.ok(entry)
  assert.match(entry.detail, /### This is not a heading/)
  assert.match(entry.detail, /Text after the fence\./)
})

test('a heading that is neither Unreleased nor a version drops its entries', () => {
  const releases = parseChangelog(SAMPLE)
  const titles = releases.flatMap(r => r.entries.map(e => e.title))
  assert.ok(!titles.includes('This entry must be dropped'))
})

test('an empty document parses to no releases', () => {
  assert.deepEqual(parseChangelog(''), [])
})

test('carriage returns do not leak into titles or details', () => {
  const releases = parseChangelog('## 1.0.0\r\n\r\n### Title\r\n\r\nDetail.\r\n')
  assert.equal(releases[0].entries[0].title, 'Title')
  assert.equal(releases[0].entries[0].detail, 'Detail.')
})

test('compareVersions orders by major, minor then patch', () => {
  assert.equal(compareVersions('0.1.23', '0.1.22'), 1)
  assert.equal(compareVersions('0.1.22', '0.1.23'), -1)
  assert.equal(compareVersions('0.2.0', '0.1.99'), 1)
  assert.equal(compareVersions('1.0.0', '0.9.9'), 1)
  assert.equal(compareVersions('0.1.23', '0.1.23'), 0)
})

test('compareVersions tolerates a v prefix and ignores a prerelease suffix', () => {
  assert.equal(compareVersions('v0.1.23', '0.1.23'), 0)
  assert.equal(compareVersions('0.1.24-rc.1', '0.1.24'), 0)
})

test('releasesSince returns only newer releases, newest first, without Unreleased', () => {
  const all = parseChangelog(SAMPLE)
  assert.deepEqual(releasesSince(all, '1.2.2').map(r => r.version), ['1.2.3'])
  assert.deepEqual(releasesSince(all, '1.2.3'), [])
  assert.deepEqual(releasesSince(all, '1.0.0').map(r => r.version), ['1.2.3', '1.2.2'])
})
