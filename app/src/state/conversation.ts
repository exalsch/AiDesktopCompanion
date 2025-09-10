// Facade re-exports from submodules
export { Role, MessageType, ImageRef, Message, Conversation, PersistedState, uid } from './conversation_types'
export { state as default, state } from './conversation_state'
export { newConversation, setCurrentConversation, getConversationsSorted, clearAllConversations } from './conversation_state'
export { appendMessage, updateMessage } from './conversation_messages'
export { getPersistState, setPersistState } from './conversation_persist'
