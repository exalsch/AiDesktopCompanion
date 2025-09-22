import { computed, reactive, ref } from 'vue'
import type { Ref } from 'vue'

export function useQuickPrompts(composerInput: Ref<string>, composerRef: Ref<any>) {
  // Quick Prompts state (read-only for main UI)
  const quickPrompts = reactive<Record<string, string>>({
    '1': '', '2': '', '3': '',
    '4': '', '5': '', '6': '',
    '7': '', '8': '', '9': ''
  })

  async function loadQuickPrompts() {
    try {
      // Lazy import to avoid tight coupling with tauri core here
      const { invoke } = await import('@tauri-apps/api/core')
      const data = await invoke<any>('get_quick_prompts')
      for (let i = 1; i <= 9; i++) {
        const k = String(i)
        ;(quickPrompts as any)[k] = (data && typeof (data as any)[k] === 'string') ? (data as any)[k] : ''
      }
    } catch (err) {
      console.warn('[quick-prompts] load failed', err)
    }
  }

  function insertQuickPrompt(i: number) {
    try {
      const k = String(i)
      const text = (quickPrompts as any)[k] || ''
      if (!text || !text.trim()) return
      const cur = composerInput.value || ''
      composerInput.value = cur ? `${text} ${cur}` : text
      requestAnimationFrame(() => { try { (composerRef.value as any)?.focus?.() } catch {} })
    } catch (e) {
      console.warn('[quick-prompts] insert failed', e)
    }
  }

  const activeQuickPrompt = ref<number | null>(null)

  const selectedSystemPrompt = computed(() => {
    const i = activeQuickPrompt.value
    if (!i) return ''
    const k = String(i)
    const text = (quickPrompts as any)[k] || ''
    return (typeof text === 'string' ? text.trim() : '')
  })

  function toggleQuickPrompt(i: number) {
    try {
      const k = String(i)
      const text = (quickPrompts as any)[k] || ''
      if (!text || !text.trim()) return
      activeQuickPrompt.value = (activeQuickPrompt.value === i) ? null : i
      requestAnimationFrame(() => { try { (composerRef.value as any)?.focus?.() } catch {} })
    } catch (e) {
      console.warn('[quick-prompts] toggle failed', e)
    }
  }

  return {
    quickPrompts,
    loadQuickPrompts,
    insertQuickPrompt,
    activeQuickPrompt,
    selectedSystemPrompt,
    toggleQuickPrompt,
  }
}
