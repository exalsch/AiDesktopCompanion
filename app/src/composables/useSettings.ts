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
  stt_local_model: 'whisper' as string,
  stt_parakeet_has_cuda: false as boolean,
  stt_cloud_base_url: 'https://api.openai.com' as string,
  stt_cloud_model: 'whisper-1' as string,
  stt_cloud_api_key: '' as string,
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
        // Preserve runtime status and object identity for existing servers.
        const existingById = new Map<string, any>(
          Array.isArray(settings.mcp_servers) ? settings.mcp_servers.map((x: any) => [String(x?.id || ''), x]) : []
        )
        const next: any[] = []
        for (const s of v.mcp_servers) {
          const id = String(s?.id || '')
          const prev = existingById.get(id)
          const envObj = normalizeEnvInput(s?.env)
          const envJsonStr = Object.keys(envObj).length ? JSON.stringify(envObj, null, 0) : '{ "LOG_LEVEL": "info" }'
          const config = {
            id,
            transport: (s.transport === 'http' || s.transport === 'sse') ? 'http' : 'stdio',
            command: String(s.command || ''),
            args: Array.isArray(s.args) ? s.args.filter((x: any) => typeof x === 'string') : [],
            argsText: Array.isArray(s.args) ? s.args.join(' ') : (typeof s.args === 'string' ? s.args : ''),
            cwd: typeof s.cwd === 'string' ? s.cwd : '',
            env: envObj,
            envJson: envJsonStr,
            auto_connect: s.auto_connect === true,
            disabled_tools: Array.isArray(s.disabled_tools) ? s.disabled_tools.filter((x: any) => typeof x === 'string') : [],
          }
          if (prev) {
            // Update config fields in place; preserve runtime fields like status/tools.
            prev.id = config.id
            prev.transport = config.transport
            prev.command = config.command
            prev.args = config.args
            prev.argsText = config.argsText
            prev.cwd = config.cwd
            prev.env = config.env
            prev.envJson = config.envJson
            prev.auto_connect = config.auto_connect
            prev.disabled_tools = config.disabled_tools
            // Ensure required runtime fields exist
            if (typeof prev.status !== 'string') prev.status = 'disconnected'
            if (typeof prev.connecting !== 'boolean') prev.connecting = false
            if (!('error' in prev)) prev.error = null
            if (!Array.isArray(prev.tools)) prev.tools = []
            if (typeof prev.toolsOpen !== 'boolean') prev.toolsOpen = false
            if (typeof prev.selectedTool !== 'string') prev.selectedTool = ''
            if (typeof prev.toolArgsJson !== 'string') prev.toolArgsJson = '{}'
            if (!('toolArgsError' in prev)) prev.toolArgsError = null
            if (!Array.isArray(prev.toolResults)) prev.toolResults = []
            if (!('envError' in prev)) prev.envError = null
            next.push(prev)
          } else {
            next.push({
              ...config,
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
            })
          }
        }
        // Replace array contents in place to preserve reactivity on the top-level ref
        settings.mcp_servers.splice(0, settings.mcp_servers.length, ...next)
      }
      // STT engine selection (optional; default openai)
      if (typeof (v as any).stt_engine === 'string') {
        const se = String((v as any).stt_engine).toLowerCase()
        settings.stt_engine = (se === 'local') ? 'local' : 'openai'
      }
      if (typeof (v as any).stt_local_model === 'string' && String((v as any).stt_local_model).trim()) {
        settings.stt_local_model = String((v as any).stt_local_model).trim()
      }
      if (typeof (v as any).stt_parakeet_has_cuda === 'boolean') {
        settings.stt_parakeet_has_cuda = (v as any).stt_parakeet_has_cuda === true
      }
      if (typeof (v as any).stt_cloud_base_url === 'string' && String((v as any).stt_cloud_base_url).trim()) {
        settings.stt_cloud_base_url = String((v as any).stt_cloud_base_url).trim()
      }
      if (typeof (v as any).stt_cloud_model === 'string' && String((v as any).stt_cloud_model).trim()) {
        settings.stt_cloud_model = String((v as any).stt_cloud_model).trim()
      }
      if (typeof (v as any).stt_cloud_api_key === 'string') {
        settings.stt_cloud_api_key = String((v as any).stt_cloud_api_key)
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

