<script setup lang="ts">
import CollapsibleCard from '../ui/CollapsibleCard.vue'

// A card's identity must not change while the user types in it.
//
// `s.id` is the server name and the very field being edited, so keying on it
// tore the card down on every keystroke: the input lost focus, and the rebuilt
// card read its saved open state under a persistence id that had changed too,
// found none, and folded. Keying on the array index instead would survive
// typing but not deletion, where every later card would inherit the previous
// one's open state. Tying identity to the object keeps both stable.
const cardKeys = new WeakMap<object, string>()
let cardKeySeq = 0

function cardKey(server: any): string {
  if (!server || typeof server !== 'object') return 'mcp-unknown'
  let key = cardKeys.get(server)
  if (!key) {
    key = `mcp-${++cardKeySeq}`
    cardKeys.set(server, key)
  }
  return key
}

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

/** Maps a server's state onto the shared status tokens, so its chip matches
 *  every other status surface in the app. */
function statusClass(s: any): string {
  if (s?.connecting) return 'warn'
  if (s?.status === 'connected') return 'ok'
  if (s?.error) return 'err'
  return ''
}

function statusLabel(s: any): string {
  if (s?.connecting) return 'connecting'
  return String(s?.status || 'disconnected')
}

function schemaProps(s: any): Record<string, any> | null {
  const tool = props.selectedToolObj(s)
  return tool?.inputSchema?.properties || tool?.input_schema?.properties || null
}
</script>

<template>
  <section class="card">
    <div class="card-body">
      <div class="actions">
        <button class="btn" type="button" @click="props.onAdd">Add server</button>
        <span class="field-hint">stdio and http transports. Changes save automatically.</span>
      </div>

      <div class="divider"></div>

      <label class="switch row">
        <input type="checkbox" v-model="props.settings.mcp_show_console" />
        <span class="switch-text">
          <span class="switch-label">Show server console windows</span>
          <span class="switch-hint">
            stdio servers run as child processes, which Windows normally gives a console window of their own.
            They are hidden because the app talks to them over pipes and there is nothing to type into.
            Turn this on when a server dies at startup and its error only appears there. Takes effect on the next connect.
          </span>
        </span>
      </label>
    </div>
  </section>

  <p v-if="props.settings.mcp_servers.length === 0" class="empty">
    No servers configured yet. Add one to give the assistant tools it can call.
  </p>

  <!-- One collapsible card per server: a machine with six of these configured
       was previously a single unbroken wall of inputs. The status chip stays
       visible in the header while the card is folded. -->
  <CollapsibleCard
    v-for="(s, i) in props.settings.mcp_servers"
    :key="cardKey(s)"
    :id="'settings.mcp.' + cardKey(s)"
    :title="s.id || 'Unnamed server'"
    :desc="s.transport === 'http' ? (s.command || 'no URL set') : (s.command || 'no command set')"
    :default-open="false"
  >
    <template #aside>
      <span class="badge" :class="statusClass(s)">
        <span class="dot" :class="statusClass(s)"></span>
        {{ statusLabel(s) }}
      </span>
    </template>

    <div class="field-grid">
      <div class="field">
        <label class="field-label">Name</label>
        <input
          class="input"
          v-model="s.id"
          placeholder="my-server"
          @input="s.id = s.id.replace(/[^a-zA-Z0-9_-]/g, '')"
        />
        <p class="field-hint">Used as the key. Letters, numbers, hyphens and underscores only.</p>
        <p v-if="s.id && s.id.includes('__')" class="field-hint error">
          Must not contain a double underscore - that is the separator used in generated tool names.
        </p>
      </div>

      <div class="field">
        <label class="field-label">Transport</label>
        <select class="input" v-model="s.transport">
          <option value="stdio">stdio</option>
          <option value="http">http</option>
        </select>
      </div>

      <div class="field" v-if="s.transport === 'http'">
        <label class="field-label">URL</label>
        <input class="input" v-model="s.command" placeholder="https://server.example.com/mcp" />
      </div>

      <div class="field" v-if="s.transport === 'stdio'">
        <label class="field-label">Command</label>
        <input class="input" v-model="s.command" placeholder="uv / node / python / server.exe" />
      </div>

      <div class="field" v-if="s.transport === 'stdio'">
        <label class="field-label">Arguments</label>
        <input class="input" v-model="s.argsText" placeholder="--flag value 'quoted arg'" />
      </div>

      <div class="field" v-if="s.transport === 'stdio'">
        <label class="field-label">Working directory</label>
        <input class="input" v-model="s.cwd" placeholder="c:\path\to\server" />
      </div>
    </div>

    <div class="field" v-if="s.transport === 'stdio'">
      <label class="field-label">Environment</label>
      <textarea
        class="input mono"
        rows="2"
        v-model="s.envJson"
        spellcheck="false"
        placeholder='{"LOG_LEVEL":"info"}'
        @input="props.onValidateEnvJsonInput(s)"
      ></textarea>
      <p class="field-hint">A JSON object, or one <code>KEY=VALUE</code> per line.</p>
      <p v-if="s.envError" class="field-hint error">{{ s.envError }}</p>
    </div>

    <label class="switch row">
      <input type="checkbox" v-model="s.auto_connect" />
      <span class="switch-text">
        <span class="switch-label">Connect on startup</span>
        <span class="switch-hint">The app connects to this server automatically when it launches.</span>
      </span>
    </label>

    <p v-if="s.error" class="field-hint error">{{ s.error }}</p>

    <div class="divider"></div>

    <div class="actions">
      <button
        class="btn"
        type="button"
        :disabled="s.connecting"
        @click="(s.status === 'connected') ? props.onDisconnect(s) : props.onConnect(s)"
      >{{ s.connecting ? 'Connecting…' : (s.status === 'connected' ? 'Disconnect' : 'Connect') }}</button>
      <button class="btn ghost" type="button" :disabled="s.status !== 'connected'" @click="props.onPing(s)">Ping</button>
      <button
        class="btn ghost"
        type="button"
        :disabled="s.status !== 'connected'"
        @click="s.toolsOpen ? (s.toolsOpen = false) : props.onListTools(s)"
      >{{ s.toolsOpen ? 'Hide tools' : 'List tools' }}</button>
      <span class="spacer"></span>
      <button class="btn danger" type="button" @click="props.onRemove(Number(i))">Remove</button>
    </div>

    <!-- Tools explorer, nested one level inside its server -->
    <section class="card nested" v-if="s.toolsOpen">
      <div class="card-head">
        <span class="card-heading">
          <span class="card-title">Tools</span>
          <span class="card-desc">{{ (s.tools || []).length }} exposed by this server</span>
        </span>
      </div>
      <div class="card-body">
        <div class="field">
          <label class="field-label">Tool</label>
          <div class="actions">
            <select class="input" v-model="s.selectedTool">
              <option value="" disabled>Select a tool</option>
              <option v-for="t in s.tools" :key="t.name || t.id || t.title" :value="t.name || t.id">{{ t.name || t.id || t.title }}</option>
            </select>
            <button class="btn" type="button" :disabled="!s.selectedTool" @click="props.onCallTool(s)">Call</button>
          </div>
          <p class="field-hint" v-if="s.selectedTool">{{ props.selectedToolObj(s)?.description || 'No description provided.' }}</p>
        </div>

        <div v-if="s.selectedTool" class="field">
          <label class="field-label">Parameters</label>
          <dl v-if="schemaProps(s)" class="schema">
            <template v-for="(prop, key) in schemaProps(s)" :key="String(key)">
              <dt>{{ key }}</dt>
              <dd>
                <span class="badge">{{ (prop as any)?.type || 'any' }}</span>
                <span v-if="(prop as any)?.description">{{ (prop as any).description }}</span>
              </dd>
            </template>
          </dl>
          <p v-else class="field-hint">No parameter schema provided.</p>
          <div class="actions">
            <button class="btn ghost sm" type="button" @click="props.onFillArgsTemplate(s)">Fill args template</button>
          </div>
        </div>

        <div class="field">
          <label class="field-label">Arguments (JSON)</label>
          <textarea class="input mono" rows="3" v-model="s.toolArgsJson" spellcheck="false"></textarea>
          <p v-if="s.toolArgsError" class="field-hint error">{{ s.toolArgsError }}</p>
        </div>

        <div class="field">
          <label class="field-label">Recent results</label>
          <p v-if="!s.toolResults || s.toolResults.length === 0" class="field-hint">Nothing called yet.</p>
          <div v-for="(r, idx) in s.toolResults" :key="idx" class="result">
            <div class="result-head">
              <span class="result-tool">{{ r.tool }}</span>
              <span class="field-hint">{{ r.at }}</span>
            </div>
            <pre class="result-body">{{ JSON.stringify(r.result, null, 2) }}</pre>
          </div>
        </div>
      </div>
    </section>
  </CollapsibleCard>
</template>

<style scoped>
/* A card inside a card: drop the shadow so the nesting reads as depth rather
   than as two floating panels. */
.nested {
  box-shadow: none;
  background: var(--adc-bg);
}

/* Parameter schema as a definition list rather than rows of spans, so the
   names form a column the eye can scan. */
.schema {
  display: grid;
  grid-template-columns: minmax(90px, max-content) 1fr;
  gap: var(--sp-1) var(--sp-3);
  margin: 0;
  font-size: var(--fs-sm);
}
.schema dt {
  font-family: var(--font-mono);
  color: var(--adc-fg);
  overflow-wrap: anywhere;
}
.schema dd {
  margin: 0;
  color: var(--adc-fg-muted);
  display: flex;
  gap: var(--sp-2);
  align-items: baseline;
  flex-wrap: wrap;
}

.result {
  border: 1px solid var(--adc-border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}
.result + .result { margin-top: var(--sp-2); }
.result-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sp-3);
  padding: var(--sp-2) var(--sp-3);
  background: var(--adc-surface-2);
}
.result-tool { font-family: var(--font-mono); font-size: var(--fs-sm); }
.result-body {
  margin: 0;
  padding: var(--sp-3);
  /* Tool output is arbitrary JSON: cap the height so one chatty result cannot
     push everything below it off the page. */
  max-height: 260px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--adc-fg-muted);
  user-select: text;
}
</style>
