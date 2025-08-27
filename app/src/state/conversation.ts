import { reactive } from 'vue'

export type Role = 'user' | 'assistant' | 'system'
export type MessageType = 'text' | 'image'

export interface ImageRef {
  path: string
  src: string // convertFileSrc(path)
}

export interface Message {
  id: string
  role: Role
  type: MessageType
  text?: string
  images?: ImageRef[]
  createdAt: number
}

export interface Conversation {
  id: string
  messages: Message[]
  createdAt?: number
  updatedAt?: number
}

function uid(prefix: string) {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`
}

const initialConv: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }
const state = reactive<{ conversations: Conversation[]; currentConversation: Conversation }>({
  conversations: [initialConv],
  currentConversation: initialConv,
})

export function newConversation() {
  const c: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }
  state.conversations.unshift(c)
  state.currentConversation = c
}

export function setCurrentConversation(id: string): boolean {
  const found = state.conversations.find((c) => c.id === id)
  if (found) {
    state.currentConversation = found
    return true
  }
  return false
}

export function getConversationsSorted(): Conversation[] {
  return [...state.conversations].sort(
    (a, b) => (b.updatedAt ?? b.createdAt ?? 0) - (a.updatedAt ?? a.createdAt ?? 0)
  )
}

export function clearAllConversations(): Conversation {
  const c: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }
  state.conversations = [c]
  state.currentConversation = c
  return c
}

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
  }
  state.currentConversation.messages.push(m)
  // bump last updated on this conversation
  const t = m.createdAt || Date.now()
  state.currentConversation.updatedAt = Math.max(state.currentConversation.updatedAt || 0, t)
  return m
}

export default state

// ---------------------------
// Persistence helpers
// ---------------------------
export interface PersistedState {
  conversations: Conversation[]
  currentId: string
}

/**
 * Returns a plain JSON-friendly snapshot of the current conversation state.
 */
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
      })),
    })),
    currentId: state.currentConversation.id,
  }
}

/**
 * Loads a previously persisted conversation state. Returns true on success.
 * Validates structure defensively to avoid runtime issues.
 */
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
      if (m.role !== 'user' && m.role !== 'assistant' && m.role !== 'system') continue
      if (m.type !== 'text' && m.type !== 'image') continue
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
      if (m.role !== 'user' && m.role !== 'assistant' && m.role !== 'system') continue
      if (m.type !== 'text' && m.type !== 'image') continue
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
