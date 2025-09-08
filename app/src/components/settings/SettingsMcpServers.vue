<script setup lang="ts">
import { defineProps } from 'vue'

const props = defineProps<{
  settings: any
  onAdd: () => void
  onRemove: (index: number) => void
  onSave: () => void
  onConnect: (s: any) => void
  onDisconnect: (s: any) => void
  onPing: (s: any) => void
  onListTools: (s: any) => void
  onFillArgsTemplate: (s: any) => void
  onValidateEnvJsonInput: (s: any) => void
  onCallTool: (s: any) => void
  selectedToolObj: (s: any) => any
}>()
</script>

<template>  
  <div class="settings-section">
    <div class="settings-title">MCP Servers</div>  
    <div class="settings-hint">Configure MCP servers. Supports stdio and http transports.</div>

    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.auto_connect"/> Auto-connect on startup</label>
    </div>
    <div class="settings-row">
      <button class="btn" @click="props.onAdd">Add Server</button>
      <button class="btn" @click="props.onSave">Save MCP Servers</button>
    </div>

    <div v-if="props.settings.mcp_servers.length === 0" class="settings-hint">No servers configured.</div>

    <div v-for="(s, i) in props.settings.mcp_servers" :key="s.id || i" class="settings-section" style="margin-top:8px;">
      <div class="settings-row">
        <label class="label" style="width:100px;">Name</label>
        <input class="input" v-model="s.id" placeholder="my-server" />
        <span class="settings-hint">Used as key for events/commands.</span>
      </div>
      <div class="settings-row">
        <label class="label" style="width:100px;">Transport</label>
        <select class="input" v-model="s.transport">
          <option value="stdio">stdio</option>
          <option value="http">http</option>
        </select>
      </div>

      <!-- URL for HTTP -->
      <div class="settings-row" v-if="s.transport === 'http'">
        <label class="label" style="width:100px;">URL</label>
        <input class="input" v-model="s.command" placeholder="https://server.example.com/mcp" />
      </div>

      <!-- stdio-only fields -->
      <div class="settings-row" v-if="s.transport === 'stdio'">
        <label class="label" style="width:100px;">Command</label>
        <input class="input" v-model="s.command" placeholder="uv / node / python / server.exe" />
      </div>
      <div class="settings-row" v-if="s.transport === 'stdio'">
        <label class="label" style="width:100px;">Args</label>
        <input class="input" v-model="s.argsText" placeholder="--flag value 'quoted arg'" />
      </div>
      <div class="settings-row" v-if="s.transport === 'stdio'">
        <label class="label" style="width:100px;">CWD</label>
        <input class="input" v-model="s.cwd" placeholder="c:\\path\\to\\server" />
      </div>
      <div class="settings-row col" v-if="s.transport === 'stdio'">
        <label class="label">Env (JSON object or KEY=VALUE lines)</label>
        <textarea
          class="input"
          rows="2"
          v-model="s.envJson"
          spellcheck="false"
          placeholder='{"LOG_LEVEL":"info"}'
          @input="props.onValidateEnvJsonInput(s)"
        ></textarea>
        <div v-if="s.envError" class="settings-hint error">{{ s.envError }}</div>
      </div>

      <div class="settings-row">
        <label class="checkbox"><input type="checkbox" v-model="s.auto_connect"/> Auto-connect this server</label>
      </div>

      <div class="settings-row" style="justify-content: space-between;">
        <div>
          <span class="label">Status:</span>
          <span style="margin-left:6px;">{{ s.status }}</span>
          <span v-if="s.error" class="settings-hint error" style="margin-left:10px;">{{ s.error }}</span>
        </div>
        <div style="display:flow; gap:8px;">
          <button class="btn" :disabled="s.connecting" @click="props.onConnect(s)">{{ s.connecting ? 'Connecting…' : 'Connect' }}</button>
          <button class="btn" @click="props.onDisconnect(s)">Disconnect</button>
          <button class="btn" @click="props.onPing(s)">Ping</button>
          <button class="btn" @click="s.toolsOpen ? (s.toolsOpen = false) : props.onListTools(s)">{{ s.toolsOpen ? 'Hide Tools' : 'List Tools' }}</button>
          <button class="btn danger" @click="props.onRemove(i)">Remove</button>
        </div>
      </div>

      <!-- Tools panel -->
      <div v-if="s.toolsOpen" class="settings-section" style="margin-top:8px;">
        <div class="settings-title">Tools</div>
        <div class="settings-row">
          <label class="label" style="width:100px;">Tool</label>
          <select class="input" v-model="s.selectedTool">
            <option value="" disabled>Select a tool</option>
            <option v-for="t in s.tools" :key="t.name || t.id || t.title" :value="t.name || t.id">{{ t.name || t.id || t.title }}</option>
          </select>
          <button class="btn" :disabled="!s.selectedTool" @click="props.onCallTool(s)">Call Tool</button>
        </div>
        <div class="settings-hint" v-if="s.selectedTool">
          {{ (props.selectedToolObj(s)?.description) || '' }}
        </div>
        <!-- Dynamic parameter fields from tool schema -->
        <div v-if="s.selectedTool" class="settings-row col" style="margin-top:4px;">
          <label class="label">Parameters</label>
          <div v-if="(props.selectedToolObj(s)?.inputSchema?.properties) || (props.selectedToolObj(s)?.input_schema?.properties)" class="settings-hint">
            <div v-for="(prop, key) in (props.selectedToolObj(s)?.inputSchema?.properties || props.selectedToolObj(s)?.input_schema?.properties)" :key="String(key)" style="margin:2px 0;">
              <span class="label" style="min-width:120px; display:inline-block;">{{ key }}</span>
              <span class="settings-hint">{{ (prop as any)?.type || '' }}</span>
              <span class="settings-hint" v-if="(prop as any)?.description"> — {{ (prop as any).description }}</span>
            </div>
          </div>
          <div v-else class="settings-hint">No parameter schema provided.</div>
          <div style="margin-top:4px;">
            <button class="btn" @click="props.onFillArgsTemplate(s)">Fill args template</button>
          </div>
        </div>
        <div class="settings-row col" style="margin-top:6px;">
          <label class="label">Args (JSON object)</label>
          <textarea class="input" rows="3" v-model="s.toolArgsJson" spellcheck="false"></textarea>
          <div v-if="s.toolArgsError" class="settings-hint error">{{ s.toolArgsError }}</div>
        </div>
        <div class="settings-row col" style="margin-top:6px;">
          <label class="label">Recent Results</label>
          <div v-if="!s.toolResults || s.toolResults.length === 0" class="settings-hint">No results yet.</div>
          <div v-for="(r, idx) in s.toolResults" :key="idx" class="settings-section" style="margin-top:6px;">
            <div class="settings-row" style="justify-content: space-between;">
              <div><span class="label">Tool:</span> {{ r.tool }}</div>
              <div class="settings-hint">{{ r.at }}</div>
            </div>
            <div class="settings-row col">
              <label class="label">Output</label>
              <pre class="input" style="white-space: pre-wrap; overflow:auto;">{{ JSON.stringify(r.result, null, 2) }}</pre>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
