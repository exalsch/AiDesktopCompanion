import { state } from './conversation_state'
import type { Message } from './conversation_types'
import { uid } from './conversation_types'

export function appendMessage(
  msg: Omit<Message, 'id' | 'createdAt'> & Partial<Pick<Message, 'id' | 'createdAt'>>
): Message {
  const m: Message = {
    id: msg.id || uid('m'),
    createdAt: msg.createdAt || Date.now(),
    role: msg.role,
    type: msg.type,
    text: msg.text,
    images: msg.images,
    tool: msg.tool,
  }
  state.currentConversation.messages.push(m)
  const t = m.createdAt || Date.now()
  state.currentConversation.updatedAt = Math.max(state.currentConversation.updatedAt || 0, t)
  return m
}

export function updateMessage(id: string, patch: Partial<Message>): Message | null {
  const list = state.currentConversation.messages
  const idx = list.findIndex(m => m.id === id)
  if (idx === -1) return null
  const cur = list[idx]
  const next: Message = {
    ...cur,
    ...patch,
    tool: (patch.tool || cur.tool) ? { ...(cur.tool || {}), ...(patch.tool || {}) } : undefined,
  }
  list[idx] = next
  state.currentConversation.updatedAt = Math.max(state.currentConversation.updatedAt || 0, Date.now())
  return next
}
