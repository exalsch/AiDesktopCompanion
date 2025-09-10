import { state } from './conversation_state'
import type { Conversation, Message, PersistedState } from './conversation_types'

export function getPersistState(): PersistedState {
  return {
    conversations: state.conversations.map((c) => ({
      id: c.id,
      createdAt: c.createdAt ?? Date.now(),
      updatedAt: c.updatedAt ?? c.createdAt ?? Date.now(),
      messages: c.messages.map((m) => ({
        id: m.id,
        createdAt: m.createdAt,
        role: m.role,
        type: m.type,
        text: m.text,
        images: m.images ? m.images.map((i) => ({ path: i.path, src: i.src })) : undefined,
        tool: m.tool ? {
          id: m.tool.id,
          function: m.tool.function,
          serverId: m.tool.serverId,
          tool: m.tool.tool,
          args: m.tool.args,
          ok: m.tool.ok,
          result: m.tool.result,
          error: m.tool.error,
          status: m.tool.status,
        } : undefined,
      })),
    })),
    currentId: state.currentConversation.id,
  }
}

export function setPersistState(v: any): boolean {
  if (!v || typeof v !== 'object') return false

  // Backward compatibility: { currentConversation }
  if (v.currentConversation && typeof v.currentConversation === 'object') {
    const cc = v.currentConversation
    if (typeof cc.id !== 'string' || !Array.isArray(cc.messages)) return false
    const safeMessages: Message[] = []
    for (const m of cc.messages) {
      if (!m || typeof m !== 'object') continue
      if (typeof m.id !== 'string') continue
      if (typeof m.createdAt !== 'number') continue
      if (m.role !== 'user' && m.role !== 'assistant' && m.role !== 'system' && m.role !== 'tool') continue
      if (m.type !== 'text' && m.type !== 'image' && m.type !== 'tool') continue
      const msg: Message = {
        id: m.id,
        createdAt: m.createdAt,
        role: m.role,
        type: m.type,
        text: typeof m.text === 'string' ? m.text : undefined,
        images: Array.isArray(m.images)
          ? m.images
              .filter((i: any) => i && typeof i.path === 'string' && typeof i.src === 'string')
              .map((i: any) => ({ path: i.path, src: i.src }))
          : undefined,
        tool: (m.type === 'tool' || m.role === 'tool') && m.tool && typeof m.tool === 'object'
          ? {
              id: typeof (m.tool as any).id === 'string' ? (m.tool as any).id : undefined,
              function: typeof (m.tool as any).function === 'string' ? (m.tool as any).function : undefined,
              serverId: typeof (m.tool as any).serverId === 'string' ? (m.tool as any).serverId : undefined,
              tool: typeof (m.tool as any).tool === 'string' ? (m.tool as any).tool : undefined,
              args: (m.tool as any).args,
              ok: typeof (m.tool as any).ok === 'boolean' ? (m.tool as any).ok : undefined,
              result: (m.tool as any).result,
              error: typeof (m.tool as any).error === 'string' ? (m.tool as any).error : undefined,
              status: ((m.tool as any).status === 'started' || (m.tool as any).status === 'finished') ? (m.tool as any).status : undefined,
            }
          : undefined,
      }
      safeMessages.push(msg)
    }
    const times = safeMessages.map((m) => m.createdAt)
    const created = times.length ? Math.min(...times) : Date.now()
    const updated = times.length ? Math.max(...times) : created
    const conv: Conversation = { id: cc.id, messages: safeMessages, createdAt: created, updatedAt: updated }
    state.conversations = [conv]
    state.currentConversation = conv
    return true
  }

  // New format: { conversations: Conversation[], currentId: string }
  const arr = Array.isArray(v.conversations) ? v.conversations : null
  const curId = typeof v.currentId === 'string' ? v.currentId : ''
  if (!arr) return false

  const list: Conversation[] = []
  for (const c of arr) {
    if (!c || typeof c !== 'object') continue
    if (typeof c.id !== 'string') continue
    const msgsIn = Array.isArray((c as any).messages) ? (c as any).messages : []
    const safeMessages: Message[] = []
    for (const m of msgsIn) {
      if (!m || typeof m !== 'object') continue
      if (typeof m.id !== 'string') continue
      if (typeof m.createdAt !== 'number') continue
      if (m.role !== 'user' && m.role !== 'assistant' && m.role !== 'system' && m.role !== 'tool') continue
      if (m.type !== 'text' && m.type !== 'image' && m.type !== 'tool') continue
      const msg: Message = {
        id: m.id,
        createdAt: m.createdAt,
        role: m.role,
        type: m.type,
        text: typeof m.text === 'string' ? m.text : undefined,
        images: Array.isArray(m.images)
          ? m.images
              .filter((i: any) => i && typeof i.path === 'string' && typeof i.src === 'string')
              .map((i: any) => ({ path: i.path, src: i.src }))
          : undefined,
        tool: (m.type === 'tool' || m.role === 'tool') && m.tool && typeof m.tool === 'object'
          ? {
              id: typeof (m.tool as any).id === 'string' ? (m.tool as any).id : undefined,
              function: typeof (m.tool as any).function === 'string' ? (m.tool as any).function : undefined,
              serverId: typeof (m.tool as any).serverId === 'string' ? (m.tool as any).serverId : undefined,
              tool: typeof (m.tool as any).tool === 'string' ? (m.tool as any).tool : undefined,
              args: (m.tool as any).args,
              ok: typeof (m.tool as any).ok === 'boolean' ? (m.tool as any).ok : undefined,
              result: (m.tool as any).result,
              error: typeof (m.tool as any).error === 'string' ? (m.tool as any).error : undefined,
              status: ((m.tool as any).status === 'started' || (m.tool as any).status === 'finished') ? (m.tool as any).status : undefined,
            }
          : undefined,
      }
      safeMessages.push(msg)
    }
    const times = safeMessages.map((m) => m.createdAt)
    const created = typeof (c as any).createdAt === 'number' ? (c as any).createdAt : (times.length ? Math.min(...times) : Date.now())
    const updated = typeof (c as any).updatedAt === 'number' ? (c as any).updatedAt : (times.length ? Math.max(...times) : created)
    list.push({ id: c.id, messages: safeMessages, createdAt: created, updatedAt: updated })
  }

  if (list.length === 0) {
    const c: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }
    state.conversations = [c]
    state.currentConversation = c
    return true
  }

  state.conversations = list
  const found = list.find((x) => x.id === curId) || list[0]
  state.currentConversation = found
  return true
}
