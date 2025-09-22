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

// Remove a conversation by id. If the current conversation is removed,
// select the next available one (most recent) or create a new one.
export function deleteConversation(id: string): boolean {
  const idx = state.conversations.findIndex(c => c.id === id)
  if (idx === -1) return false

  const deleted = state.conversations.splice(idx, 1)[0]

  // If no conversations left, create a fresh one
  if (state.conversations.length === 0) {
    const c: Conversation = { id: uid('c'), messages: [], createdAt: Date.now(), updatedAt: Date.now() }
    state.conversations.push(c)
    state.currentConversation = c
    return true
  }

  // If we deleted the current conversation, select the most recently updated remaining
  if (deleted && state.currentConversation.id === deleted.id) {
    const next = getConversationsSorted()[0]
    if (next) state.currentConversation = next
  }
  return true
}

