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

onMounted(loadPrompts)
</script>

<template>
  <div class="qp-editor">
    <div class="qp-header">
      <div class="title">Quick Prompts Editor</div>
      <div class="actions">
        <button class="btn" :disabled="busy" @click="save">Save</button>
        <button class="btn secondary" :disabled="busy" @click="resetDefaults">Reset to defaults</button>
      </div>
    </div>

    <div v-if="!loaded && !err" class="hint">Loading…</div>
    <div v-if="err" class="error">{{ err }}</div>

    <div class="grid">
      <div v-for="i in 9" :key="i" class="cell">
        <label>Prompt for key {{ i }}</label>
        <textarea v-model="form[String(i)]" rows="3" class="input"/>
      </div>
    </div>
  </div>
</template>

<style scoped>
.qp-editor { margin-top: 12px; }
.qp-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
.title { font-weight: 700; }
.actions { display: flex; gap: 8px; }
.btn { padding: 6px 10px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-accent); color: #fff; cursor: pointer; }
.btn.secondary { background: transparent; color: var(--adc-fg); }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.hint { color: #9fa0aa; margin-bottom: 8px; }
.error { color: #ff9b9b; margin-bottom: 8px; white-space: pre-line; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 10px; }
.cell { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
label { font-size: 12px; color: var(--adc-fg-muted); }
textarea { width: 100%; resize: vertical; min-height: 70px; padding: 8px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); box-sizing: border-box; }
</style>
