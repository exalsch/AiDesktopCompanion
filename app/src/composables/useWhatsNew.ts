import { ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import changelogMd from '../../../CHANGELOG.md?raw'
import {
  parseChangelog,
  compareVersions,
  releasesSince,
  type ChangelogRelease,
} from '../changelog/parse'

/**
 * The What's New dialog: what changed in the versions this machine skipped.
 *
 * The changelog is inlined into the bundle by Vite's `?raw`, so there is no
 * fetch at runtime and nothing to add to the CSP.
 *
 * The seen marker lives in localStorage rather than settings.json for the same
 * reason the update-check verdict does: it is a per-machine view preference,
 * not something worth round-tripping through the Rust settings layer, whose
 * `save_settings` allowlist would have to grow a key for it.
 */

const STORAGE_KEY = 'adc.whatsNew.lastSeenVersion'

// Parsed once at module load. The file is a few kilobytes and the result is
// shared by every caller.
const allReleases = parseChangelog(changelogMd)

const visible = ref(false)
const releases = ref<ChangelogRelease[]>([])
const currentVersion = ref('')
let started = false

function readSeen(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY)
  } catch {
    // Private mode or a locked-down webview. Treated as a fresh install, which
    // means the dialog stays quiet rather than reappearing every launch.
    return null
  }
}

function writeSeen(version: string) {
  try {
    localStorage.setItem(STORAGE_KEY, version)
  } catch {}
}

async function init() {
  const current = await getVersion().catch(() => '')
  if (!current) return
  currentVersion.value = current

  const seen = readSeen()
  if (!seen) {
    // A fresh install. Nobody upgraded, so there is nothing to announce; just
    // record where this machine starts from.
    writeSeen(current)
    return
  }

  // A stored version at or ahead of the running one means either nothing new or
  // a downgrade. Both correct themselves by re-stamping the running version.
  if (compareVersions(seen, current) >= 0) {
    writeSeen(current)
    return
  }

  const since = releasesSince(allReleases, seen)
  if (since.length === 0) {
    // Upgraded across versions that documented nothing. Do not open an empty
    // dialog; just catch the marker up.
    writeSeen(current)
    return
  }

  releases.value = since
  visible.value = true
}

/** Reopen on demand, showing every documented release. Does not touch the marker. */
function open() {
  releases.value = allReleases.filter(r => r.version !== 'Unreleased')
  visible.value = true
}

function dismiss() {
  visible.value = false
  if (currentVersion.value) writeSeen(currentVersion.value)
}

export function useWhatsNew() {
  if (!started) {
    started = true
    void init()
  }
  return { visible, releases, currentVersion, open, dismiss }
}
