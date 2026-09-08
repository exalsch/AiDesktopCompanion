<script setup lang="ts">
/**
 * What changed since the version this machine last ran.
 *
 * Opens itself once after an update and is reachable afterwards from the
 * version label in the sidebar. Entries arrive collapsed: the headline is what
 * most people want, the detail is one click away.
 */
import { onMounted, onBeforeUnmount, ref, watch } from 'vue'
import CollapsibleCard from './ui/CollapsibleCard.vue'
import { useWhatsNew } from '../composables/useWhatsNew'
import { initMarkdownSingleton } from '../markdown'
import type { ChangelogEntry } from '../changelog/parse'

const { visible, releases, dismiss } = useWhatsNew()

const panel = ref<HTMLElement | null>(null)
const rendered = ref<Record<string, string>>({})
// Where focus was before the dialog took it, so closing puts it back rather
// than dumping the user at the top of the document.
let restoreFocusTo: HTMLElement | null = null

const KIND_LABEL: Record<string, string> = {
  feat: 'New',
  fix: 'Fixed',
  perf: 'Faster',
}

// The global `.badge` carries the shape; only a new feature earns a colour.
const KIND_CLASS: Record<string, string> = {
  feat: 'badge ok',
  fix: 'badge',
  perf: 'badge',
}

function kindLabel(entry: ChangelogEntry): string {
  return entry.kind ? KIND_LABEL[entry.kind] : ''
}

function kindClass(entry: ChangelogEntry): string {
  return entry.kind ? KIND_CLASS[entry.kind] : ''
}

/** Stable per-entry key: the version scopes titles that repeat across releases. */
function entryKey(version: string, index: number): string {
  return `${version}#${index}`
}

function hasDetail(entry: ChangelogEntry): boolean {
  return entry.detail.trim().length > 0
}

/**
 * Render every detail once the dialog opens. Done eagerly rather than on expand
 * so opening a card never flashes empty while markdown-it loads.
 */
async function renderDetails() {
  const render = await initMarkdownSingleton()
  const next: Record<string, string> = {}
  for (const release of releases.value) {
    release.entries.forEach((entry, i) => {
      if (hasDetail(entry)) next[entryKey(release.version, i)] = render(entry.detail)
    })
  }
  rendered.value = next
}

watch(visible, async (open) => {
  if (!open) {
    restoreFocusTo?.focus?.()
    restoreFocusTo = null
    return
  }
  restoreFocusTo = document.activeElement instanceof HTMLElement ? document.activeElement : null
  await renderDetails()
  // Focus the panel so Escape and Tab land inside the dialog rather than the
  // page behind it.
  requestAnimationFrame(() => panel.value?.focus())
}, { immediate: true })

function onKey(e: KeyboardEvent) {
  if (!visible.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    dismiss()
  }
}

onMounted(() => { window.addEventListener('keydown', onKey) })
onBeforeUnmount(() => { window.removeEventListener('keydown', onKey) })
</script>

<template>
  <div v-if="visible" class="overlay" @click.self="dismiss()">
    <div
      ref="panel"
      class="panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby="whats-new-title"
      tabindex="-1"
    >
      <header class="panel-head">
        <div>
          <h2 id="whats-new-title" class="panel-title">What's new</h2>
          <p class="panel-desc">Changes since the version you were running.</p>
        </div>
        <button class="btn ghost sm icon close" type="button" title="Close (Esc)" @click="dismiss()">✕</button>
      </header>

      <div class="panel-body">
        <section v-for="release in releases" :key="release.version" class="release">
          <h3 class="release-head">
            <span class="release-version">v{{ release.version }}</span>
            <span v-if="release.date" class="release-date">{{ release.date }}</span>
          </h3>

          <p v-if="release.entries.length === 0" class="release-empty">
            No user-facing changes were recorded for this version.
          </p>

          <template v-for="(entry, i) in release.entries" :key="entryKey(release.version, i)">
            <CollapsibleCard v-if="hasDetail(entry)" :title="entry.title" :default-open="false">
              <template v-if="entry.kind" #aside>
                <span :class="kindClass(entry)">{{ kindLabel(entry) }}</span>
              </template>
              <!-- Sanitised by DOMPurify inside the shared renderer. -->
              <div class="detail" v-html="rendered[entryKey(release.version, i)] || ''"></div>
            </CollapsibleCard>

            <!-- Nothing to expand into, so it renders as a plain row rather than
                 a card with a chevron that does nothing. -->
            <div v-else class="card flat-entry">
              <span class="flat-title">{{ entry.title }}</span>
              <span v-if="entry.kind" :class="kindClass(entry)">{{ kindLabel(entry) }}</span>
            </div>
          </template>
        </section>
      </div>

      <footer class="panel-foot">
        <button class="btn" type="button" @click="dismiss()">Got it</button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  padding: var(--sp-4);
  box-sizing: border-box;
}

.panel {
  width: min(680px, 100%);
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  background: var(--adc-bg);
  border: 1px solid var(--adc-border);
  border-radius: var(--radius);
  box-shadow: var(--shadow-2);
  overflow: hidden;
}
.panel:focus { outline: none; }

.panel-head {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-3);
  padding: var(--sp-4);
  border-bottom: 1px solid var(--adc-border);
}
.panel-title { margin: 0; font-size: var(--fs-lg); }
.panel-desc { margin: 4px 0 0; font-size: var(--fs-sm); color: var(--adc-fg-muted); }
.close { margin-left: auto; }

.panel-body {
  overflow-y: auto;
  padding: var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}

.release-head {
  margin: 0 0 var(--sp-2);
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
  font-size: var(--fs-base);
}
.release-version { font-weight: 600; }
.release-date { font-size: var(--fs-sm); color: var(--adc-fg-muted); }
.release-empty { margin: 0; font-size: var(--fs-sm); color: var(--adc-fg-muted); }

.release :deep(.card) { margin-bottom: var(--sp-2); }

.flat-entry {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: 10px var(--sp-3);
}
.flat-title { flex: 1; }

.detail { font-size: var(--fs-base); line-height: 1.55; }
.detail :deep(p) { margin: 0 0 var(--sp-2); }
.detail :deep(p:last-child) { margin-bottom: 0; }
.detail :deep(ul), .detail :deep(ol) { margin: 0 0 var(--sp-2); padding-left: 20px; }
.detail :deep(code) { font-size: var(--fs-sm); }
.detail :deep(pre) { overflow-x: auto; }

.panel-foot {
  display: flex;
  justify-content: flex-end;
  padding: var(--sp-3) var(--sp-4);
  border-top: 1px solid var(--adc-border);
}
</style>
