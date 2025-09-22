export type Role = 'user' | 'assistant' | 'system' | 'tool'
export type MessageType = 'text' | 'image' | 'tool'

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
  tool?: {
    id?: string
    function?: string
    serverId?: string
    tool?: string
    args?: any
    ok?: boolean
    result?: any
    error?: string
    status?: 'started' | 'finished'
  }
  createdAt: number
}

export interface Conversation {
  id: string
  messages: Message[]
  createdAt?: number
  updatedAt?: number
}

export function uid(prefix: string) {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`
}

export interface PersistedState {
  conversations: Conversation[]
  currentId: string
}
