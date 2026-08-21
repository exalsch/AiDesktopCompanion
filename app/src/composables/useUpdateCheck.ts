import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'

/**
 * Notify-only update check.
 *
 * Asks the backend whether a newer GitHub release exists and exposes enough to
 * render a link. Nothing is downloaded or installed - the user opens the release
 * page and decides.
 */

export interface UpdateInfo {
  current: string
  latest: string
  url: string
  update_available: boolean
}

// Keyed by the running version: a verdict is only meaningful for the build that
// asked for it, and an installer replaces the binary without touching
// localStorage.
const CACHE_KEY_PREFIX = 'adc.updateCheck.'

// GitHub allows 60 unauthenticated requests an hour per IP. Checking a few times
// a day is plenty for a release cadence measured in days.
const CACHE_TTL_MS = 6 * 60 * 60 * 1000

async function cacheKey(): Promise<string> {
  try {
    return CACHE_KEY_PREFIX + (await getVersion())
  } catch {
    return CACHE_KEY_PREFIX + 'unknown'
  }
}

// Module-level so every component that calls this shares one result and one
// request, rather than each mount hitting the API.
const info = ref<UpdateInfo | null>(null)
const checking = ref(false)
let started = false

async function readCache(): Promise<UpdateInfo | null> {
  try {
    const raw = localStorage.getItem(await cacheKey())
    if (!raw) return null
    const parsed = JSON.parse(raw)
    if (!parsed || typeof parsed.at !== 'number') return null
    if (Date.now() - parsed.at > CACHE_TTL_MS) return null
    const cached: UpdateInfo | null = parsed.info ?? null
    if (!cached) return null
    // Second line of defence: the key should already have ruled this out, but a
    // verdict that disagrees with the running build is worse than no verdict -
    // it can advertise an update that is actually a downgrade.
    const running = await getVersion().catch(() => '')
    if (running && cached.current && cached.current !== running) return null
    return cached
  } catch {
    return null
  }
}

async function writeCache(value: UpdateInfo) {
  try {
    localStorage.setItem(await cacheKey(), JSON.stringify({ at: Date.now(), info: value }))
  } catch {
    // Private mode or a full quota. The check just runs again next time.
  }
}

/// Drop verdicts left behind by other versions, so upgrading does not slowly
/// fill localStorage with stale keys.
function pruneOtherVersions(keep: string) {
  try {
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const k = localStorage.key(i)
      if (k && k.startsWith(CACHE_KEY_PREFIX) && k !== keep) localStorage.removeItem(k)
    }
    // The unversioned key written before this was keyed at all.
    localStorage.removeItem('adc.updateCheck')
  } catch {}
}

/**
 * Run the check, honouring the cache unless `force` is set.
 *
 * Failures are swallowed on purpose: being offline is not worth interrupting
 * anyone over, and the version label simply stays as it is.
 */
async function check(force = false) {
  if (checking.value) return
  const key = await cacheKey()
  pruneOtherVersions(key)
  if (!force) {
    const cached = await readCache()
    if (cached) { info.value = cached; return }
  }
  checking.value = true
  try {
    const result = await invoke<UpdateInfo>('check_for_update')
    info.value = result
    await writeCache(result)
  } catch {
    // Leave whatever we had; no update badge is the correct fallback.
  } finally {
    checking.value = false
  }
}

async function openRelease() {
  const url = info.value?.url
  if (!url) return
  try {
    await invoke('open_release_page', { url })
  } catch {
    // The backend refuses anything outside this project's repository.
  }
}

export function useUpdateCheck() {
  // First caller kicks the check off; later ones reuse the result.
  if (!started) {
    started = true
    void check()
  }
  return { info, checking, check, openRelease }
}
