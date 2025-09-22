import { watch } from 'vue'
import type { Ref } from 'vue'
import conversation, { getPersistState, setPersistState } from '../state/conversation'
import { invoke } from '@tauri-apps/api/core'

export function useConversationPersist(settingsPersistConversations: Ref<boolean>, showToast: (msg: string, kind?: 'error'|'success', ms?: number) => void) {
  async function loadPersistedConversation() {
    if (!settingsPersistConversations.value) return
    try {
      const v = await invoke<any>('load_conversation_state')
      if (v && typeof v === 'object' && Object.keys(v).length > 0) {
        const ok = setPersistState(v)
        if (!ok) showToast('Failed to load conversation history.', 'error')
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : (err && err.message) ? err.message : 'Unknown error'
      showToast(`Failed to load conversation history: ${msg}`, 'error')
    }
  }

  let saveDebounce: any = 0
  function schedulePersistSave() {
    if (!settingsPersistConversations.value) return
    if (saveDebounce) clearTimeout(saveDebounce)
    saveDebounce = setTimeout(async () => {
      try {
        await invoke<string>('save_conversation_state', { state: getPersistState() })
      } catch (e) {
        console.warn('[persist] save failed', e)
      }
    }, 300)
  }

  function registerConversationPersist() {
    const stopFns: Array<() => void> = []
    // Persist when messages change
    stopFns.push(watch(() => conversation.currentConversation.messages.length, () => schedulePersistSave()))
    // Persist when switching current conversation (so currentId is saved)
    stopFns.push(watch(() => conversation.currentConversation.id, () => schedulePersistSave()))
    // Persist when conversations are added/removed
    stopFns.push(watch(() => conversation.conversations.length, () => schedulePersistSave()))
    return () => { try { stopFns.forEach(s => s()) } catch {} }
  }

  return { loadPersistedConversation, registerConversationPersist }
}
