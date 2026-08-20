import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

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

const CACHE_KEY = 'adc.updateCheck'
// GitHub allows 60 unauthenticated requests an hour per IP. Checking a few times
// a day is plenty for a release cadence measured in days.
const CACHE_TTL_MS = 6 * 60 * 60 * 1000

// Module-level so every component that calls this shares one result and one
// request, rather than each mount hitting the API.
const info = ref<UpdateInfo | null>(null)
const checking = ref(false)
let started = false

function readCache(): UpdateInfo | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw)
    if (!parsed || typeof parsed.at !== 'number') return null
    if (Date.now() - parsed.at > CACHE_TTL_MS) return null
    return parsed.info ?? null
  } catch {
    return null
  }
}

function writeCache(value: UpdateInfo) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), info: value }))
  } catch {
    // Private mode or a full quota. The check just runs again next time.
  }
}

/**
 * Run the check, honouring the cache unless `force` is set.
 *
 * Failures are swallowed on purpose: being offline is not worth interrupting
 * anyone over, and the version label simply stays as it is.
 */
async function check(force = false) {
  if (checking.value) return
  if (!force) {
    const cached = readCache()
    if (cached) { info.value = cached; return }
  }
  checking.value = true
  try {
    const result = await invoke<UpdateInfo>('check_for_update')
    info.value = result
    writeCache(result)
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
