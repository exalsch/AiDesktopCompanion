import { invoke } from '@tauri-apps/api/core'
import type { UIStyle } from './useSettings'
import { parseArgs, normalizeEnvInput, parseJsonObject } from './utils'

export function useMcp(settings: any, showToast: (msg: string, kind?: 'error' | 'success', ms?: number) => void) {
  function findServerById(id: string) {
    return settings.mcp_servers.find((s: any) => s.id === id)
  }

  async function connectServer(s: any) {
    if (!s || !s.id) { showToast('Server name is required', 'error'); return }
    try {
      s.connecting = true; s.error = null
      const args = s.transport === 'stdio'
        ? parseArgs(typeof s.argsText === 'string' ? s.argsText : (Array.isArray(s.args) ? s.args.join(' ') : ''))
        : []
      const env = s.transport === 'stdio'
        ? normalizeEnvInput(typeof s.envJson === 'string' ? s.envJson : s.env)
        : {}
      const res = await invoke<string>('mcp_connect', {
        serverId: s.id,
        command: s.command,
        args,
        cwd: s.transport === 'stdio' ? (s.cwd || null) : null,
        env,
        transport: s.transport
      })
      // If backend says it's already connected, we won't get another mcp:connected event.
      // Recover UI state immediately to avoid appearing disconnected.
      if (typeof res === 'string' && res.toLowerCase().includes('already connected')) {
        s.status = 'connected'
        s.connecting = false
        s.error = null
      }
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
      s.error = msg
      showToast(`Connect failed: ${msg}`, 'error')
    } finally {
      s.connecting = false
    }
  }

  async function disconnectServer(s: any) {
    if (!s || !s.id) return
    try {
      await invoke<string>('mcp_disconnect', { serverId: s.id })
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
      showToast(`Disconnect failed: ${msg}`, 'error')
    }
  }

  async function pingServer(s: any) {
    if (!s || !s.id) return
    try {
      const res = await invoke<string>('mcp_ping', { serverId: s.id })
      showToast(`Ping: ${res}`, 'success', 1500)
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
      showToast(`Ping failed: ${msg}`, 'error')
    }
  }

  async function listTools(s: any) {
    if (!s || !s.id) return
    try {
      const v = await invoke<any>('mcp_list_tools', { serverId: s.id })
      const tools = Array.isArray(v?.tools) ? v.tools : (Array.isArray(v) ? v : [])
      s.tools = tools
      s.toolsOpen = true
      const count = Array.isArray(tools) ? tools.length : 0
      showToast(`Tools: ${count} loaded.`, 'success', 1500)
      console.log('[mcp] list_tools', v)
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
      showToast(`list_tools failed: ${msg}`, 'error')
    }
  }

  function onListTools(serverId: string) {
    const s = findServerById(serverId)
    if (s) listTools(s)
  }

  function onToggleTool(payload: { serverId: string; tool: string; enabled: boolean }) {
    const s = findServerById(payload.serverId)
    if (!s || !payload.tool) return
    const name = String(payload.tool)
    s.disabled_tools = Array.isArray(s.disabled_tools) ? s.disabled_tools : []
    const idx = s.disabled_tools.indexOf(name)
    if (payload.enabled) {
      if (idx !== -1) s.disabled_tools.splice(idx, 1)
    } else {
      if (idx === -1) s.disabled_tools.push(name)
    }
  }

  function validateEnvJsonInput(s: any) {
    try {
      const env = normalizeEnvInput(typeof s.envJson === 'string' ? s.envJson : s.env)
      if (typeof s.envJson === 'string' && s.envJson.trim() && Object.keys(env).length === 0) {
        s.envError = 'Could not parse ENV. Use JSON like {"KEY":"VALUE"} or lines KEY=VALUE'
      } else {
        s.envError = null
      }
    } catch (e) {
      s.envError = (e as Error).message
    }
  }

  async function callTool(s: any) {
    if (!s || !s.id) return
    if (!s.selectedTool) { showToast('Select a tool first', 'error'); return }
    try {
      s.toolArgsError = null
      const args = (typeof s.toolArgsJson === 'string' && s.toolArgsJson.trim()) ? parseJsonObject(s.toolArgsJson) : {}
      const res = await invoke<any>('mcp_call_tool', { serverId: s.id, name: s.selectedTool, args })
      const entry = { tool: s.selectedTool, args, result: res, at: new Date().toISOString() }
      s.toolResults = [entry, ...(Array.isArray(s.toolResults) ? s.toolResults : [])].slice(0, 10)
      showToast('Tool executed.', 'success', 1200)
      console.log('[mcp] call_tool', entry)
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
      s.toolArgsError = msg
      showToast(`call_tool failed: ${msg}`, 'error')
    }
  }

  function selectedToolObj(s: any) {
    try {
      return (Array.isArray(s.tools) ? s.tools : []).find((t: any) => (t.name || t.id) === s.selectedTool)
    } catch { return null }
  }

  return {
    findServerById,
    connectServer,
    disconnectServer,
    pingServer,
    listTools,
    onListTools,
    onToggleTool,
    validateEnvJsonInput,
    callTool,
    selectedToolObj,
  }
}
