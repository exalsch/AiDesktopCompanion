<script setup lang="ts">
import { reactive, ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const props = defineProps<{ notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void }>()

const form = reactive<Record<string, string>>({
  '1': '', '2': '', '3': '',
  '4': '', '5': '', '6': '',
  '7': '', '8': '', '9': ''
})

const busy = ref(false)
const loaded = ref(false)
const err = ref('')

async function loadPrompts() {
  busy.value = true
  err.value = ''
  try {
    const data = await invoke<any>('get_quick_prompts')
    if (!data || typeof data !== 'object') throw new Error('Invalid response')
    for (let i = 1; i <= 9; i++) {
      const k = String(i)
      const v = typeof data[k] === 'string' ? data[k] : ''
      form[k] = v
    }
    loaded.value = true
  } catch (e: any) {
    err.value = e?.message || String(e) || 'Failed to load quick prompts'
  } finally {
    busy.value = false
  }
}

async function save() {
  busy.value = true
  err.value = ''
  try {
    const map: Record<string, string> = {}
    for (let i = 1; i <= 9; i++) {
      const k = String(i)
      map[k] = form[k] ?? ''
    }
    await invoke<string>('save_quick_prompts', { map })
    props.notify?.('Quick Prompts saved successfully', 'success')
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Failed to save quick prompts'
    err.value = msg
    props.notify?.(`Save failed: ${msg}`, 'error')
  } finally {
    busy.value = false
  }
}

async function resetDefaults() {
  busy.value = true
  err.value = ''
  try {
    await invoke<string>('generate_default_quick_prompts')
    await loadPrompts()
    props.notify?.('Defaults generated and loaded', 'success')
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Failed to generate defaults'
    err.value = msg
    props.notify?.(`Defaults failed: ${msg}`, 'error')
  } finally {
    busy.value = false
  }
}

// Expose methods to parent for external Save/Reset triggers
defineExpose({ save, resetDefaults, loadPrompts })

onMounted(loadPrompts)
</script>

<template>
  <div class="qp-editor">
    <p v-if="!loaded && !err" class="field-hint">Loading…</p>
    <p v-if="err" class="field-hint error">{{ err }}</p>

    <div class="grid">
      <div v-for="i in 9" :key="i" class="field">
        <label class="field-label" :for="'qp-' + i">
          <span class="key">{{ i }}</span>
          Prompt
        </label>
        <textarea :id="'qp-' + i" v-model="form[String(i)]" rows="3" class="input" />
      </div>
    </div>

    <div class="actions">
      <button class="btn" type="button" :disabled="busy" @click="save">Save prompts</button>
      <button class="btn ghost" type="button" :disabled="busy" @click="resetDefaults">Reset to defaults</button>
    </div>
  </div>
</template>

<style scoped>
.qp-editor { display: flex; flex-direction: column; gap: var(--sp-4); }

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: var(--sp-3);
}

/* The number is the thing you actually press, so it gets the emphasis rather
   than being buried in a "Prompt for key 3" sentence. */
.key {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: var(--radius-sm);
  background: var(--adc-surface-2);
  border: 1px solid var(--adc-border);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--adc-fg);
}

.qp-editor textarea.input { min-height: 72px; }
</style>
