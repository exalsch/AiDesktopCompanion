import { reactive } from 'vue'
import type { Conversation } from './conversation_types'
import { uid } from './conversation_types'

const initialConv: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }

export const state = reactive<{ conversations: Conversation[]; currentConversation: Conversation }>({
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
  if (found) { state.currentConversation = found; return true }
  return false
}

export function getConversationsSorted(): Conversation[] {
  return [...state.conversations].sort((a, b) => (b.updatedAt ?? b.createdAt ?? 0) - (a.updatedAt ?? a.createdAt ?? 0))
}

export function clearAllConversations(): Conversation {
  const c: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }
  state.conversations = [c]
  state.currentConversation = c
  return c
}
