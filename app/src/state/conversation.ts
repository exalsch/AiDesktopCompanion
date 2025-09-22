// Facade re-exports from submodules
export type { Role, MessageType, ImageRef, Message, Conversation, PersistedState } from './conversation_types'
export { uid } from './conversation_types'
export { state as default, state } from './conversation_state'
export { newConversation, setCurrentConversation, getConversationsSorted, clearAllConversations, deleteConversation } from './conversation_state'
export { appendMessage, updateMessage } from './conversation_messages'
export { getPersistState, setPersistState } from './conversation_persist'
