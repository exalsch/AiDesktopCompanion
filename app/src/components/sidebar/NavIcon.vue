<script setup lang="ts">
/**
 * The sidebar's icon set, keyed by name.
 *
 * These were previously inlined into SidebarNav's markup, which forced the
 * four Settings sub-items to be written out as four near-identical blocks.
 * Pulling them here lets that nav be described as data.
 *
 * All icons share a 24x24 viewBox and are stroked with `currentColor`, so they
 * take the colour of whatever nav item holds them - including the white of an
 * active item - with no per-state overrides.
 */
export type NavIconName =
  | 'menu'
  | 'prompt'
  | 'assistant'
  | 'tts'
  | 'stt'
  | 'settings'
  | 'history'
  | 'general'
  | 'quick-prompts'
  | 'mcp'

defineProps<{ name: NavIconName }>()
</script>

<template>
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <template v-if="name === 'menu'">
      <line x1="3" y1="6" x2="21" y2="6" />
      <line x1="3" y1="12" x2="21" y2="12" />
      <line x1="3" y1="18" x2="21" y2="18" />
    </template>

    <template v-else-if="name === 'prompt'">
      <path d="m12 19 7-7 3 3-7 7-3-3z" />
      <path d="m18 13-1.5-7.5L2 2l3.5 14.5L13 18l5-5z" />
      <path d="m2 2 7.586 7.586" />
      <circle cx="11" cy="11" r="2" />
    </template>

    <template v-else-if="name === 'assistant'">
      <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
      <rect x="3" y="18" width="18" height="3" rx="1.5" />
      <path d="M8 21v-3" />
      <path d="M16 21v-3" />
    </template>

    <template v-else-if="name === 'tts'">
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
    </template>

    <template v-else-if="name === 'stt'">
      <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
      <line x1="12" x2="12" y1="19" y2="22" />
    </template>

    <template v-else-if="name === 'settings'">
      <circle cx="12" cy="12" r="3" />
      <rect x="11" y="0" width="2" height="4" rx="1" />
      <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(60 12 12)" />
      <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(120 12 12)" />
      <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(180 12 12)" />
      <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(240 12 12)" />
      <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(300 12 12)" />
    </template>

    <template v-else-if="name === 'history'">
      <circle cx="12" cy="12" r="8" />
      <path d="M12 8v4l3 3" />
      <path d="M3 12a9 9 0 1 0 9-9" />
      <polyline points="3 12 3 7 8 7" />
    </template>

    <template v-else-if="name === 'general'">
      <line x1="4" y1="21" x2="4" y2="14" />
      <line x1="4" y1="10" x2="4" y2="3" />
      <line x1="12" y1="21" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12" y2="3" />
      <line x1="20" y1="21" x2="20" y2="16" />
      <line x1="20" y1="12" x2="20" y2="3" />
      <line x1="2" y1="14" x2="6" y2="14" />
      <line x1="10" y1="8" x2="14" y2="8" />
      <line x1="18" y1="16" x2="22" y2="16" />
    </template>

    <template v-else-if="name === 'quick-prompts'">
      <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
    </template>

    <template v-else-if="name === 'mcp'">
      <rect x="2" y="2" width="20" height="8" rx="2" />
      <rect x="2" y="14" width="20" height="8" rx="2" />
      <line x1="6" y1="6" x2="6.01" y2="6" />
      <line x1="6" y1="18" x2="6.01" y2="18" />
    </template>
  </svg>
</template>
