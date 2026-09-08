/**
 * Parser for the repository-root `CHANGELOG.md`.
 *
 * Deliberately a line scanner rather than a markdown AST walk: the grammar is
 * two heading levels and one optional metadata line, and keeping it dependency
 * free is what lets `node --test` run this file directly.
 *
 * Malformed input degrades instead of throwing. A popup that shows nothing is
 * an acceptable failure mode; a main window that will not mount is not.
 */

export type ChangelogKind = 'feat' | 'fix' | 'perf'

export interface ChangelogEntry {
  title: string
  /** Absent unless the entry carried a recognised `kind:` line. */
  kind?: ChangelogKind
  /** Markdown, may be empty. */
  detail: string
}

export interface ChangelogRelease {
  /** `Unreleased`, or a semver string. */
  version: string
  /** ISO date. Absent for `Unreleased` and for a version written without one. */
  date?: string
  entries: ChangelogEntry[]
}

const KINDS = new Set<string>(['feat', 'fix', 'perf'])

/** `## Unreleased` or `## 0.1.23 - 2026-09-07`. The date is optional. */
const RELEASE_HEADING = /^##\s+(Unreleased|\d+\.\d+\.\d+[0-9A-Za-z.+-]*)\s*(?:-\s*(\d{4}-\d{2}-\d{2}))?\s*$/
const ENTRY_HEADING = /^###\s+(.+?)\s*$/
const ANY_H2 = /^##\s+/
const ANY_H1 = /^#\s+/
const FENCE = /^\s*(```|~~~)/
/** Metadata, not prose: stripped whether or not the value is one we know. */
const KIND_LINE = /^kind:\s*([A-Za-z]+)\s*$/

export function parseChangelog(markdown: string): ChangelogRelease[] {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n')
  const releases: ChangelogRelease[] = []

  let release: ChangelogRelease | null = null
  let entry: ChangelogEntry | null = null
  let detail: string[] = []
  let inFence = false
  let fenceMarker = ''

  function flushEntry() {
    const current = entry
    entry = null
    const body = detail
    detail = []
    if (!current || !release) return

    let i = 0
    while (i < body.length && body[i].trim() === '') i++
    const meta = body[i]?.match(KIND_LINE)
    const rest = meta ? body.slice(i + 1) : body
    if (meta && KINDS.has(meta[1].toLowerCase())) {
      current.kind = meta[1].toLowerCase() as ChangelogKind
    }

    current.detail = rest.join('\n').trim()
    release.entries.push(current)
  }

  for (const line of lines) {
    const fence = line.match(FENCE)
    if (fence) {
      // Only a matching marker closes the block, so ``` inside a ~~~ block is
      // content rather than a terminator.
      if (!inFence) {
        inFence = true
        fenceMarker = fence[1]
      } else if (line.trim().startsWith(fenceMarker)) {
        inFence = false
        fenceMarker = ''
      }
      if (entry) detail.push(line)
      continue
    }

    if (!inFence) {
      const heading = line.match(RELEASE_HEADING)
      if (heading) {
        flushEntry()
        release = { version: heading[1], entries: [] }
        if (heading[2]) release.date = heading[2]
        releases.push(release)
        continue
      }
      if (ANY_H2.test(line)) {
        // An `##` we do not understand ends the current release, so the entries
        // beneath it are dropped rather than filed under the wrong version.
        flushEntry()
        release = null
        continue
      }
      const entryHeading = line.match(ENTRY_HEADING)
      if (entryHeading) {
        flushEntry()
        entry = release ? { title: entryHeading[1], detail: '' } : null
        continue
      }
      if (ANY_H1.test(line)) {
        // The document title, and any prose under it, belongs to no release.
        flushEntry()
        continue
      }
    }

    if (entry) detail.push(line)
  }

  flushEntry()
  return releases
}

/**
 * Compare two semver strings by major, minor and patch only.
 *
 * A prerelease suffix is ignored on purpose: `0.1.24-rc.1` and `0.1.24` show
 * the same changelog section, which is what a release candidate is for.
 */
export function compareVersions(a: string, b: string): number {
  const parts = (v: string) =>
    v.replace(/^v/i, '').split('-')[0].split('.').map(n => Number.parseInt(n, 10) || 0)
  const pa = parts(a)
  const pb = parts(b)
  for (let i = 0; i < 3; i++) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0)
    if (diff !== 0) return diff < 0 ? -1 : 1
  }
  return 0
}

/** Releases strictly newer than `seen`, newest first. `Unreleased` is dropped. */
export function releasesSince(all: ChangelogRelease[], seen: string): ChangelogRelease[] {
  return all
    .filter(r => r.version !== 'Unreleased' && compareVersions(r.version, seen) > 0)
    .sort((x, y) => compareVersions(y.version, x.version))
}
