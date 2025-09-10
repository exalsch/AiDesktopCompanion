import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { normalizeEnvInput } from './utils'

export type UIStyle = 'sidebar-dark' | 'sidebar-light'

export function useSettings() {
  const settings = reactive({
    openai_api_key: '',
    openai_chat_model: 'gpt-4o-mini',
    temperature: 1.0 as number,
    persist_conversations: false as boolean,
    hide_tool_calls_in_chat: false as boolean,
    ui_style: 'sidebar-dark' as UIStyle,
    global_hotkey: '' as string,
    auto_connect: false as boolean,
    mcp_servers: [] as Array<any>,
  })

  const models = reactive<{ list: string[]; loading: boolean; error: string | null }>({ list: [], loading: false, error: null })

  async function loadSettings() {
    const v = await invoke<any>('get_settings')
    if (v && typeof v === 'object') {
      if (typeof v.openai_api_key === 'string') settings.openai_api_key = v.openai_api_key
      if (typeof v.openai_chat_model === 'string' && v.openai_chat_model.trim()) settings.openai_chat_model = v.openai_chat_model
      if (typeof v.temperature === 'number') settings.temperature = v.temperature
      if (typeof v.persist_conversations === 'boolean') settings.persist_conversations = v.persist_conversations
      if (typeof (v as any).hide_tool_calls_in_chat === 'boolean') settings.hide_tool_calls_in_chat = (v as any).hide_tool_calls_in_chat
      if (typeof (v as any).global_hotkey === 'string') settings.global_hotkey = (v as any).global_hotkey
      {
        let ui: any = (v as any).ui_style
        if (ui === 'sidebar') ui = 'sidebar-dark'
        if (ui === 'light') ui = 'sidebar-light'
        if (ui === 'tabs') ui = 'sidebar-dark'
        if (ui === 'sidebar-dark' || ui === 'sidebar-light') settings.ui_style = ui
      }
      if (typeof (v as any).auto_connect === 'boolean') settings.auto_connect = (v as any).auto_connect
      if (Array.isArray(v.mcp_servers)) {
        settings.mcp_servers = v.mcp_servers.map((s: any) => {
          const envObj = normalizeEnvInput(s?.env)
          const envJsonStr = Object.keys(envObj).length ? JSON.stringify(envObj, null, 0) : '{ "LOG_LEVEL": "info" }'
          return {
            id: String(s.id || ''),
            transport: (s.transport === 'http' || s.transport === 'sse') ? 'http' : 'stdio',
            command: String(s.command || ''),
            args: Array.isArray(s.args) ? s.args.filter((x: any) => typeof x === 'string') : [],
            argsText: Array.isArray(s.args) ? s.args.join(' ') : (typeof s.args === 'string' ? s.args : ''),
            cwd: typeof s.cwd === 'string' ? s.cwd : '',
            env: envObj,
            envJson: envJsonStr,
            auto_connect: s.auto_connect === true,
            disabled_tools: Array.isArray(s.disabled_tools) ? s.disabled_tools.filter((x: any) => typeof x === 'string') : [],
            status: 'disconnected',
            connecting: false,
            error: null as string | null,
            tools: [],
            toolsOpen: false,
            selectedTool: '',
            toolArgsJson: '{}',
            toolArgsError: null as string | null,
            toolResults: [] as Array<any>,
            envError: null as string | null,
          }
        })
      }
    }
  }

  return { settings, models, loadSettings }
}
