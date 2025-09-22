import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { normalizeEnvInput } from './utils'

export type UIStyle = 'sidebar-dark' | 'sidebar-light'

// Module-singleton state to ensure all components share the same settings instance
const DEFAULT_SYSTEM_PROMPT = (
  'For every user prompt, follow these steps internally before responding:\n' +
  '1. Analyze Intent: What is the user\'s core question or need?\n' +
  '2. Assess Knowledge: Can I answer this accurately and completely using my existing training data?\n' +
  '3. Evaluate Tools: If my knowledge is insufficient, review the available tools. Is there a tool that is a perfect match for the user\'s need?\n' +
  '4. Decide Action:\n' +
  '   - If a tool is necessary, select it and call it with the correct parameters.\n' +
  '   - If no tool is necessary, formulate a direct answer using your internal knowledge.'
)

const settings = reactive({
  openai_api_key: '',
  openai_chat_model: 'gpt-4o-mini',
  quick_prompt_model: '' as string,
  temperature: 1.0 as number,
  persist_conversations: false as boolean,
  hide_tool_calls_in_chat: false as boolean,
  ui_style: 'sidebar-dark' as UIStyle,
  global_hotkey: '' as string,
  mcp_servers: [] as Array<any>,
  system_prompt: '' as string,
  quick_prompt_system_prompt: 'Give the direct response to the task.' as string,
  show_quick_prompt_result_in_popup: false as boolean,
  tokenizer_mode: 'approx' as 'approx' | 'tiktoken',
  stt_engine: 'openai' as 'openai' | 'local',
  // Local Whisper (STT) model config
  stt_whisper_model_preset: 'base' as string,
  stt_whisper_model_url: '' as string,
})

const models = reactive<{ list: string[]; loading: boolean; error: string | null }>({ list: [], loading: false, error: null })

export function useSettings() {
  async function loadSettings() {
    const v = await invoke<any>('get_settings')
    if (v && typeof v === 'object') {
      if (typeof v.openai_api_key === 'string') settings.openai_api_key = v.openai_api_key
      if (typeof v.openai_chat_model === 'string' && v.openai_chat_model.trim()) settings.openai_chat_model = v.openai_chat_model
      // Optional dedicated model for quick prompts via Quick Actions; empty means fallback to global
      if (typeof (v as any).quick_prompt_model === 'string') {
        settings.quick_prompt_model = (v as any).quick_prompt_model
      } else {
        settings.quick_prompt_model = ''
      }
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
      // System prompt: default to sensible instructions when missing
      if (typeof (v as any).system_prompt === 'string') {
        settings.system_prompt = (v as any).system_prompt
      } else {
        settings.system_prompt = DEFAULT_SYSTEM_PROMPT
      }
      // Optional override for Quick Prompts system prompt
      if (typeof (v as any).quick_prompt_system_prompt === 'string') {
        settings.quick_prompt_system_prompt = (v as any).quick_prompt_system_prompt
      } else {
        settings.quick_prompt_system_prompt = 'Give the direct response to the task.'
      }
      // Show Quick Prompt result in Quick Actions popup (optional; default false)
      if (typeof (v as any).show_quick_prompt_result_in_popup === 'boolean') {
        settings.show_quick_prompt_result_in_popup = (v as any).show_quick_prompt_result_in_popup
      } else {
        settings.show_quick_prompt_result_in_popup = false
      }
      // Tokenizer mode (optional; defaults to approximate)
      if (typeof (v as any).tokenizer_mode === 'string') {
        const tm = String((v as any).tokenizer_mode).toLowerCase()
        settings.tokenizer_mode = (tm === 'tiktoken') ? 'tiktoken' : 'approx'
      }
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
      // STT engine selection (optional; default openai)
      if (typeof (v as any).stt_engine === 'string') {
        const se = String((v as any).stt_engine).toLowerCase()
        settings.stt_engine = (se === 'local') ? 'local' : 'openai'
      }
      // Whisper model selection (optional)
      if (typeof (v as any).stt_whisper_model_preset === 'string') {
        settings.stt_whisper_model_preset = (v as any).stt_whisper_model_preset
      }
      if (typeof (v as any).stt_whisper_model_url === 'string') {
        settings.stt_whisper_model_url = (v as any).stt_whisper_model_url
      }
    }
  }

  return { settings, models, loadSettings }
}

