<script setup lang="ts">
/**
 * A `.card` whose body can be folded away.
 *
 * Give it a stable `id` and the open/closed choice survives a restart, kept in
 * localStorage rather than settings.json: it is a per-machine view preference,
 * not something worth syncing or round-tripping through the Rust settings
 * layer.
 *
 * The header is a real <button> so it is reachable by keyboard and announces
 * its state. The body stays mounted when closed (v-show, not v-if) so a
 * half-typed value in a collapsed section is not thrown away.
 */
import { ref, watch } from 'vue'

const props = withDefaults(defineProps<{
  title: string
  desc?: string
  /** Stable key for persisting the open/closed state. Omit to not persist. */
  id?: string
  defaultOpen?: boolean
}>(), {
  defaultOpen: true,
})

const STORAGE_PREFIX = 'adc.collapse.'

function initialOpen(): boolean {
  if (!props.id) return props.defaultOpen
  try {
    const raw = localStorage.getItem(STORAGE_PREFIX + props.id)
    if (raw === '0') return false
    if (raw === '1') return true
  } catch {
    // Private mode or a locked-down webview: fall through to the default.
  }
  return props.defaultOpen
}

const open = ref(initialOpen())

watch(open, (v) => {
  if (!props.id) return
  try { localStorage.setItem(STORAGE_PREFIX + props.id, v ? '1' : '0') } catch {}
})
</script>

<template>
  <section class="card collapsible" :class="{ closed: !open }">
    <button
      type="button"
      class="card-head trigger"
      :aria-expanded="open"
      @click="open = !open"
    >
      <svg class="chevron" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="m6 4 4 4-4 4" />
      </svg>
      <span class="card-heading">
        <span class="card-title">{{ props.title }}</span>
        <span v-if="props.desc" class="card-desc">{{ props.desc }}</span>
      </span>
      <!-- Status chips, counts, anything that should stay readable while the
           section is folded. Clicks here must not toggle the card. -->
      <span v-if="$slots.aside" class="head-aside" @click.stop>
        <slot name="aside" />
      </span>
    </button>

    <div v-show="open" class="card-body">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.trigger {
  width: 100%;
  appearance: none;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.trigger:hover { background: var(--adc-hover); }
.trigger:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--adc-accent);
}

.chevron {
  width: 16px;
  height: 16px;
  flex: 0 0 auto;
  color: var(--adc-fg-muted);
  transform: rotate(90deg);
  transition: transform 0.18s ease;
}
.closed .chevron { transform: rotate(0deg); }

.head-aside {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex: 0 0 auto;
  cursor: default;
}

/* A closed card is just its header, so drop the body's top padding rule and
   let the header own the full height. */
.closed .card-head { min-height: 42px; }

@media (prefers-reduced-motion: reduce) {
  .chevron { transition: none; }
}
</style>
