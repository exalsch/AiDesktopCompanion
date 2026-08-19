<script setup lang="ts">
// Reusable global-hotkey picker: up to two modifiers plus a key.
//
// Emits the composed shortcut in plugin format ('Win' is stored as 'Super') and
// validates availability against the OS, so a shortcut another application
// already owns is reported before it is saved.
import { ref, watch, computed, onBeforeUnmount } from 'vue'
import { checkShortcutAvailable } from '../../hotkeys'

const props = defineProps<{
  /// Current shortcut, e.g. "Alt+Shift+A". Empty string means disabled.
  modelValue: string
  /// Placeholder for the key input.
  keyPlaceholder?: string
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const modOptions = [
  { label: 'None', value: '' },
  { label: 'Alt', value: 'Alt' },
  { label: 'Shift', value: 'Shift' },
  { label: 'Ctrl', value: 'Control' },
  { label: 'Win', value: 'Win' }, // will be normalized to 'Super'
]

const mod1 = ref<string>('')
const mod2 = ref<string>('')
const keyName = ref<string>('')
const error = ref<string | null>(null)
const checking = ref<boolean>(false)

// Filtered modifier options to prevent selecting the same modifier in both dropdowns (except "None")
const modOptions1 = computed(() => (mod2.value ? modOptions.filter(o => o.value !== mod2.value) : modOptions))
const modOptions2 = computed(() => (mod1.value ? modOptions.filter(o => o.value !== mod1.value) : modOptions))

// Ensure selections do not end up identical (e.g., when loading existing settings)
watch(mod1, (v) => { if (v && v === mod2.value) mod2.value = '' })
watch(mod2, (v) => { if (v && v === mod1.value) mod1.value = '' })

function parseHotkeyToFields(hk: string) {
  try {
    const s = (hk || '').trim()
    if (!s) { mod1.value = ''; mod2.value = ''; keyName.value = ''; return }
    const parts = s.split('+').map(p => p.trim()).filter(Boolean)
    const mods: string[] = []
    let key = ''
    for (const p of parts) {
      const up = p.toLowerCase()
      if (up === 'alt' || up === 'shift' || up === 'control' || up === 'ctrl' || up === 'win' || up === 'super' || up === 'command' || up === 'cmd') {
        // normalize ctrl synonyms
        const norm = (up === 'ctrl') ? 'Control' : (up === 'super' ? 'Win' : (up === 'cmd' || up === 'command') ? 'Control' : p.charAt(0).toUpperCase() + p.slice(1))
        mods.push(norm)
      } else {
        key = p
      }
    }
    mod1.value = mods[0] || ''
    mod2.value = mods[1] || ''
    // Dedupe in case both parsed modifiers are identical
    if (mod1.value && mod1.value === mod2.value) mod2.value = ''
    keyName.value = key
  } catch { mod1.value = ''; mod2.value = ''; keyName.value = '' }
}

function composeHotkey(): string {
  const mods = [mod1.value, mod2.value]
    .map(m => m.trim())
    .filter(Boolean)
    .map(m => m === 'Win' ? 'Super' : m) // normalize here for storage/consistency
  const key = (keyName.value || '').trim()
  return [...mods, key].filter(Boolean).join('+')
}

// Initialize from the current value, and keep in sync when it changes externally
parseHotkeyToFields(props.modelValue || '')
watch(() => props.modelValue, (v: string) => {
  if ((v || '') === composeHotkey()) return
  parseHotkeyToFields(typeof v === 'string' ? v : '')
})

let checkTimer: any = 0
async function validate() {
  const hk = composeHotkey()
  emit('update:modelValue', hk)
  error.value = null
  if (!hk) return
  checking.value = true
  try {
    const ok = await checkShortcutAvailable(hk)
    if (!ok) error.value = 'Hotkey appears unavailable or already in use.'
  } catch {
    error.value = 'Could not validate hotkey.'
  } finally {
    checking.value = false
  }
}

watch([mod1, mod2, keyName], () => {
  if (checkTimer) clearTimeout(checkTimer)
  checkTimer = setTimeout(validate, 300)
})

onBeforeUnmount(() => { if (checkTimer) clearTimeout(checkTimer) })
</script>

<template>
  <div class="hotkey-picker">
    <div class="hotkey-parts">
      <select v-model="mod1" class="input" aria-label="First modifier">
        <option v-for="opt in modOptions1" :key="'m1-'+opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <span class="plus" aria-hidden="true">+</span>
      <select v-model="mod2" class="input" aria-label="Second modifier">
        <option v-for="opt in modOptions2" :key="'m2-'+opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <span class="plus" aria-hidden="true">+</span>
      <input
        v-model="keyName"
        class="input key"
        :placeholder="props.keyPlaceholder || 'A or F9'"
        aria-label="Key"
        autocomplete="off"
        spellcheck="false"
      />
    </div>
    <p v-if="checking" class="field-hint">Checking availability…</p>
    <p v-if="error" class="field-hint error">{{ error }}</p>
  </div>
</template>

<style scoped>
.hotkey-picker { display: flex; flex-direction: column; gap: var(--sp-2); }

/* Modifier + modifier + key reads as one control, so the parts sit on a single
   row separated by literal plus signs rather than as three loose dropdowns. */
.hotkey-parts {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.hotkey-parts > .input { width: 150px; flex: 0 0 auto; }
.hotkey-parts > .input.key { width: 110px; }
.plus {
  color: var(--adc-fg-muted);
  font-size: var(--fs-sm);
  user-select: none;
}
</style>
