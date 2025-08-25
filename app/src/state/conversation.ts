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
}

function uid(prefix: string) {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`
}

const state = reactive<{ currentConversation: Conversation }>({
  currentConversation: { id: uid('c'), messages: [] },
})

export function newConversation() {
  state.currentConversation = { id: uid('c'), messages: [] }
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
  return m
}

export default state
