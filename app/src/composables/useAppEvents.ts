import { listen } from '@tauri-apps/api/event'
import { convertFileSrc } from '@tauri-apps/api/core'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'

// Types for dependency injection
export interface UseAppEventsDeps {
  // UI state
  prompt: { visible: boolean; selection: string; preview: string; length: number }
  ui: { activeSection: 'Prompt'|'TTS'|'STT'|'Settings'; promptSubview: 'Chat'|'History' }

  // Refs and helpers
  ttsRef: { value: any }
  composerInput: { value: string }
  composerRef: { value: any }

  // Conversation actions
  appendMessage: (msg: any) => void
  newConversation: () => void
  updateMessage: (id: string, patch: any) => boolean

  // MCP
  findServerById: (id: string) => any

  // Notifier
  showToast: (msg: string, kind?: 'error'|'success', ms?: number) => void

  // Section helper
  setSection: (s: 'Prompt' | 'TTS' | 'STT' | 'Settings') => void
}

export function useAppEvents(deps: UseAppEventsDeps) {
  async function registerAppEvents() {
    const {
      prompt, ui, ttsRef, composerInput, composerRef,
      appendMessage, newConversation, updateMessage,
      findServerById, showToast, setSection,
    } = deps

    const unsubs: Array<() => void> = []

    // Prompt open (legacy preview panel)
    const u1 = await listen<{ selection: string; preview: string; length: number }>('prompt:open', (e) => {
      const p = (e?.payload as any) || {}
      prompt.selection = typeof p.selection === 'string' ? p.selection : ''
      prompt.preview = typeof p.preview === 'string' ? p.preview : prompt.selection.slice(0, 200)
      prompt.length = typeof p.length === 'number' ? p.length : prompt.preview.length
      prompt.visible = true
    })
    unsubs.push(u1)

    // Quick Prompts config error surfaced to UI
    const u2 = await listen<{ message: string; path?: string }>('settings:quick-prompts-error', (e) => {
      const p = (e?.payload as any) || {}
      const where = p.path ? `\n${p.path}` : ''
      showToast(`Quick Prompts config error: ${p.message}${where}`, 'error')
    })
    unsubs.push(u2)

    // Image capture -> add as pending attachment (thumbnail) near composer; do not add to conversation yet
    const u3 = await listen<{ path: string }>('image:capture', async (e) => {
      const p = (e?.payload as any) || {}
      if (!p.path) return
      showToast(`Image captured:\n${p.path}`, 'success')
      try {
        const ow = await WebviewWindow.getByLabel('capture-overlay')
        if (ow) await ow.close()
      } catch {}
      try {
        const src = convertFileSrc(p.path)
        // Route to composer attachments UI
        ui.activeSection = 'Prompt'
        try { (composerRef.value as any)?.addImage?.(p.path, src) } catch {}
      } catch {}
    })
    unsubs.push(u3)

    // Direct insert into Prompt composer
    const u4 = await listen<{ text: string }>('prompt:insert', (e) => {
      const p = (e?.payload as any) || {}
      if (typeof p.text === 'string') composerInput.value = p.text
    })
    unsubs.push(u4)

    // New conversation + insert prompt text
    const u5 = await listen<{ text: string }>('prompt:new-conversation', (e) => {
      const p = (e?.payload as any) || {}
      const text = typeof p.text === 'string' ? p.text : ''
      prompt.visible = false
      newConversation()
      setSection('Prompt')
      composerInput.value = text
      requestAnimationFrame(() => { try { (composerRef.value as any)?.focus?.() } catch {} })
    })
    unsubs.push(u5)

    // TTS open with optional autoplay
    const u6 = await listen<{ text: string; autoplay?: boolean }>('tts:open', (e) => {
      const p = (e?.payload as any) || {}
      const text = typeof p.text === 'string' ? p.text : ''
      const autoplay = !!p.autoplay
      ui.activeSection = 'TTS'
      requestAnimationFrame(() => {
        const c = ttsRef.value as any
        if (!c) return
        if (autoplay) c.setTextAndPlay(text)
        else { c.setText(text); showToast('Text inserted into TTS. Press Play to start.', 'success', 1800) }
      })
    })
    unsubs.push(u6)

    // TTS errors surfaced from backend (Quick Actions TTS or TTS panel)
    const uErr = await listen<{ message: string }>('tts:error', (e) => {
      const p = (e?.payload as any) || {}
      const msg = typeof p.message === 'string' && p.message.trim() ? p.message : 'TTS error'
      showToast(msg, 'error')
    })
    unsubs.push(uErr)

    // MCP connection lifecycle
    const u7 = await listen<{ serverId: string }>('mcp:connected', (e) => {
      const id = (e?.payload as any)?.serverId
      const s = id ? findServerById(id) : null
      if (s) { s.status = 'connected'; s.error = null; s.connecting = false; showToast(`MCP connected: ${id}`, 'success', 1500) }
    })
    unsubs.push(u7)
    const u8 = await listen<{ serverId: string; existed?: boolean }>('mcp:disconnected', (e) => {
      const id = (e?.payload as any)?.serverId
      const s = id ? findServerById(id) : null
      if (s) { s.status = 'disconnected'; s.connecting = false; showToast(`MCP disconnected: ${id}`, 'success', 1500) }
    })
    unsubs.push(u8)
    const u9 = await listen<{ serverId: string; message: string }>('mcp:error', (e) => {
      const p: any = e?.payload || {}
      const s = p.serverId ? findServerById(p.serverId) : null
      if (s) s.error = p.message || 'Unknown error'
      showToast(`MCP error: ${p.message || 'Unknown error'}`, 'error')
    })
    unsubs.push(u9)

    // Chat tool call lifecycle events
    const u10 = await listen<any>('chat:tool-call', (e) => {
      try {
        const p: any = e?.payload || {}
        const id: string = typeof p.id === 'string' ? p.id : ''
        const mid = id ? `tool_${id}` : undefined
        const patch = {
          role: 'tool' as const,
          type: 'tool' as const,
          tool: {
            id,
            function: typeof p.function === 'string' ? p.function : undefined,
            serverId: typeof p.serverId === 'string' ? p.serverId : undefined,
            tool: typeof p.tool === 'string' ? p.tool : undefined,
            args: p.args,
            status: 'started' as const,
          }
        }
        if (mid) {
          const updated = updateMessage(mid, patch as any)
          if (!updated) appendMessage({ id: mid, ...patch })
        } else {
          appendMessage(patch as any)
        }
      } catch (err) {
        console.warn('[chat] tool-call event handling failed', err)
      }
    })
    unsubs.push(u10)

    const u11 = await listen<any>('chat:tool-result', (e) => {
      try {
        const p: any = e?.payload || {}
        const id: string = typeof p.id === 'string' ? p.id : ''
        const mid = id ? `tool_${id}` : ''
        const patch = {
          role: 'tool' as const,
          type: 'tool' as const,
          tool: {
            id,
            function: typeof p.function === 'string' ? p.function : undefined,
            serverId: typeof p.serverId === 'string' ? p.serverId : undefined,
            tool: typeof p.tool === 'string' ? p.tool : undefined,
            ok: p.ok === true,
            result: (p.ok === true) ? p.result : undefined,
            error: (p.ok === false && typeof p.error === 'string') ? p.error : (typeof p.error === 'string' ? p.error : undefined),
            status: 'finished' as const,
          }
        }
        const updated = mid ? updateMessage(mid, patch as any) : null
        if (!updated) appendMessage({ id: mid || undefined, ...patch })
        if (p && p.ok === false && typeof p.error === 'string' && p.error.toLowerCase().includes('disabled')) {
          const sid = typeof p.serverId === 'string' ? p.serverId : 'server'
          const tname = typeof p.tool === 'string' ? p.tool : 'tool'
          showToast(`MCP tool blocked by settings: ${sid}/${tname}`, 'error', 2500)
        }
      } catch (err) {
        console.warn('[chat] tool-result event handling failed', err)
      }
    })
    unsubs.push(u11)

    return () => { try { unsubs.forEach(u => u()) } catch {} }
  }

  return { registerAppEvents }
}
