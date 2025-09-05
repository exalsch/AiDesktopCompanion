<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import type { Message } from '../state/conversation'
import 'highlight.js/styles/github-dark.css'
import { emit as emitTauri } from '@tauri-apps/api/event'

const props = defineProps<{ message: Message; hideToolDetails?: boolean; isPlaying?: boolean }>()
const emit = defineEmits<{
  (e: 'image-click', payload: { images: { path: string; src: string }[]; index: number }): void
}>()

// Markdown renderer function ref. It is initialized dynamically to avoid
// hard dependency on external libraries at build time.
const renderMarkdown = ref<(src: string) => string>(() => basicMarkdownFallback(''))

onMounted(async () => {
  try {
    // Dynamically import markdown-it and dompurify if available
    const [{ default: MarkdownIt }, { default: DOMPurify }] = await Promise.all([
      import('markdown-it'),
      import('dompurify'),
    ])

    // Try to import highlight.js and a theme (optional)
    let hljs: any | undefined
    try {
      const mod = await import('highlight.js/lib/common')
      hljs = (mod as any).default || (mod as any)
    } catch {
      hljs = undefined
    }

    const md = new MarkdownIt({
      linkify: true,
      breaks: true,
      highlight: (code: string, lang: string) => {
        try {
          if (hljs) {
            if (lang && hljs.getLanguage(lang)) {
              return `<pre class="hljs"><code>${hljs.highlight(code, { language: lang }).value}</code></pre>`
            }
            return `<pre class="hljs"><code>${hljs.highlightAuto(code).value}</code></pre>`
          }
        } catch {}
        return `<pre class="md-pre"><code>${escapeHtml(code)}</code></pre>`
      },
    })
    renderMarkdown.value = (src: string) => DOMPurify.sanitize(md.render(src))
  } catch {
    // Fallback keeps app functional without dependencies installed
    renderMarkdown.value = (src: string) => basicMarkdownFallback(src)
  }
})

// Copy and Read Aloud actions
const copied = ref(false)

async function copyMessage() {
  try {
    if (props.message.type !== 'text') return
    const text = String(props.message.text ?? '')
    if (!text) return
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(text)
    } else {
      // Fallback: hidden textarea
      const ta = document.createElement('textarea')
      ta.value = text
      ta.style.position = 'fixed'
      ta.style.opacity = '0'
      document.body.appendChild(ta)
      ta.focus()
      ta.select()
      try { document.execCommand('copy') } catch {}
      document.body.removeChild(ta)
    }
    copied.value = true
    setTimeout(() => (copied.value = false), 1200)
  } catch (err) {
    console.error('Copy failed:', err)
  }
}

async function playTtsInBackground() {
  try {
    if (props.message.type !== 'text') return
    const text = markdownToPlain(String(props.message.text ?? ''))
    if (!text) return
    await emitTauri('tts:play-background', { text, id: props.message.id })
  } catch (err) {
    console.error('Failed to trigger TTS:', err)
  }
}

async function stopTtsInBackground() {
  try {
    await emitTauri('tts:stop-background')
  } catch (err) {
    console.error('Failed to stop TTS:', err)
  }
}

const renderedHtml = computed(() => {
  if (props.message.type !== 'text') return ''
  const src = String(props.message.text ?? '')
  return renderMarkdown.value(src)
})

// Basic, safe fallback renderer (very limited Markdown support):
// - Escapes HTML
// - Headings (# .. ######)
// - Inline code `code` and fenced code blocks ```
// - Bold **text** and italic *text*
// - Links [text](http...) (http/https only)
// - Bullet lists starting with - or *
function basicMarkdownFallback(input: string): string {
  const escape = (s: string) => s
    .replaceAll(/&/g, '&amp;')
    .replaceAll(/</g, '&lt;')
    .replaceAll(/>/g, '&gt;')

  let s = input ?? ''
  s = s.replace(/\r\n/g, '\n')

  // Fenced code blocks ```
  s = s.replace(/```([\s\S]*?)```/g, (_, code: string) => {
    return `<pre class="md-pre"><code>${escape(code)}</code></pre>`
  })

  // Headings
  s = s.replace(/^######\s+(.+)$/gm, '<h6 class="md-h">$1</h6>')
       .replace(/^#####\s+(.+)$/gm, '<h5 class="md-h">$1</h5>')
       .replace(/^####\s+(.+)$/gm, '<h4 class="md-h">$1</h4>')
       .replace(/^###\s+(.+)$/gm, '<h3 class="md-h">$1</h3>')
       .replace(/^##\s+(.+)$/gm, '<h2 class="md-h">$1</h2>')
       .replace(/^#\s+(.+)$/gm, '<h1 class="md-h">$1</h1>')

  // Lists (very minimal: consecutive lines starting with - or *)
  s = s.replace(/(?:^(?:-|\*)\s+.+\n?)+/gm, (block: string) => {
    const items = block.trim().split(/\n/).map(l => l.replace(/^(?:-|\*)\s+/, ''))
    return `<ul class="md-ul">${items.map(it => `<li>${escape(it)}</li>`).join('')}</ul>`
  })

  // Inline code
  s = s.replace(/`([^`]+)`/g, '<code class="md-code">$1</code>')

  // Bold and italic
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
       .replace(/(^|[^*])\*([^*]+)\*/g, '$1<em>$2</em>')

  // Links [text](http...)
  s = s.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>')

  // Paragraph breaks
  s = s.split(/\n\n+/).map(p => `<p>${p.replace(/\n/g, '<br/>')}</p>`).join('')

  return s
}

// Small helper for escaping code when highlight.js is unavailable
function escapeHtml(s: string): string {
  return (s ?? '')
    .replaceAll(/&/g, '&amp;')
    .replaceAll(/</g, '&lt;')
    .replaceAll(/>/g, '&gt;')
}

// Convert markdown to a plain, readable string for TTS
function markdownToPlain(input: string): string {
  let s = input ?? ''
  s = s.replace(/```[\s\S]*?```/g, '') // remove fenced code blocks
  s = s.replace(/`([^`]+)`/g, '$1') // inline code backticks
  s = s.replace(/!\[[^\]]*\]\([^\)]+\)/g, '') // images
  s = s.replace(/\[([^\]]+)\]\(([^\)]+)\)/g, '$1') // links keep text
  s = s.replace(/^>+\s?/gm, '') // blockquotes
  s = s.replace(/^#{1,6}\s*/gm, '') // headings
  s = s.replace(/\*\*([^*]+)\*\*/g, '$1') // bold
  s = s.replace(/(^|[^*])\*([^*]+)\*/g, '$1$2') // italic
  s = s.replace(/^\s*[-*]\s+/gm, '• ') // lists to bullets
  s = s.replace(/\n{3,}/g, '\n\n')
  return s.trim()
}
</script>

<template>
  <div class="row" :class="props.message.role">
    <div class="bubble" :data-type="props.message.type">
      <div v-if="props.message.type === 'text'" class="bubble-actions">
        <button class="bubble-action-btn" :title="copied ? 'Copied' : 'Copy'" @click="copyMessage" aria-label="Copy message">
          <span v-if="!copied">📋</span>
          <span v-else>✅</span>
        </button>
        <button
          v-if="!props.isPlaying"
          class="bubble-action-btn"
          title="Read aloud"
          @click="playTtsInBackground"
          aria-label="Read aloud"
        >
          <span>🔊</span>
        </button>
        <button
          v-else
          class="bubble-action-btn"
          title="Stop"
          @click="stopTtsInBackground"
          aria-label="Stop read aloud"
        >
          <span>⏹️</span>
        </button>
      </div>
      <div v-if="props.message.type === 'text'" class="text">
        <div class="md-content" v-html="renderedHtml"></div>
      </div>
      <div v-else-if="props.message.type === 'image'" class="images">
        <img v-for="(img, i) in props.message.images || []"
             :key="img.path"
             :src="img.src"
             alt="Captured image"
             class="thumb"
             @click="emit('image-click', { images: (props.message.images || []), index: i })"
        />
      </div>
      <div v-else-if="props.message.type === 'tool'" class="tool">
        <div class="tool-header">
          <span class="tool-name">{{ props.message.tool?.serverId || 'mcp' }} › {{ props.message.tool?.tool || props.message.tool?.function }}</span>
          <span class="status" :data-ok="props.message.tool?.ok === true" :data-finished="props.message.tool?.status === 'finished'">
            {{ props.message.tool?.status === 'finished' ? (props.message.tool?.ok ? 'ok' : 'error') : 'running' }}
          </span>
        </div>
        <template v-if="!props.hideToolDetails">
          <div v-if="props.message.tool?.args" class="section">
            <div class="label">args</div>
            <pre class="code">{{ JSON.stringify(props.message.tool?.args, null, 2) }}</pre>
          </div>
          <div v-if="props.message.tool?.ok && props.message.tool?.result !== undefined" class="section">
            <div class="label">result</div>
            <pre class="code">{{ JSON.stringify(props.message.tool?.result, null, 2) }}</pre>
          </div>
          <div v-else-if="props.message.tool?.status === 'finished' && props.message.tool?.error" class="section">
            <div class="label">error</div>
            <pre class="code error">{{ props.message.tool?.error }}</pre>
          </div>
        </template>
      </div>
      <div class="meta-line">
        <span class="time">{{ new Date(props.message.createdAt).toLocaleTimeString() }}</span>
        <span v-if="props.message.type === 'image'" class="badge">Image</span>
        <span v-else-if="props.message.type === 'tool'" class="badge">Tool</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Row aligns bubble left/right */
.row { display: flex; padding: 6px 10px; }
.row.assistant { justify-content: flex-start; }
.row.user { justify-content: flex-end; }

/* Bubble styles */
.bubble {
  max-width: 70%;
  background: var(--adc-surface);
  border: 1px solid var(--adc-border);
  color: var(--adc-fg);
  border-radius: 16px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  position: relative;
}
.row.assistant > .bubble {
  background: var(--adc-assistant-bubble-bg, var(--adc-surface));
  border-color: var(--adc-assistant-bubble-border, var(--adc-border));
  color: var(--adc-assistant-bubble-fg, var(--adc-fg));
  border-top-left-radius: 6px;
}
.row.user > .bubble {
  background: var(--adc-accent);
  border-color: var(--adc-accent);
  color: #ffffff;
  border-top-right-radius: 6px;
}

/* Text inside bubbles is always left-aligned */
.text { white-space: normal; text-align: left; }
.md-content { line-height: 1.45; }
.md-content :where(h1,h2,h3,h4,h5,h6) { margin: 0.6em 0 0.3em; font-weight: 700; }
.md-content h1 { font-size: 1.04em; }
.md-content h2 { font-size: 1.02em; }
.md-content h3 { font-size: 1.00em; }
.md-content p { margin: 0.4em 0; }
.md-content ul { margin: 0.4em 0 0.4em 1.2em; }
.md-content li { margin: 0.2em 0; }
.md-content a { color: var(--adc-accent); text-decoration: underline; }
.md-content code.md-code, .md-content code { background: rgba(0,0,0,0.2); padding: 0.12em 0.35em; border-radius: 6px; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; font-size: 0.95em; }
.md-content pre.md-pre, .md-content pre { background: #0b0b0b; color: #e8e8e8; border: 1px solid var(--adc-border); border-radius: 8px; padding: 8px 10px; overflow: auto; }
.md-content pre.md-pre > code, .md-content pre > code { background: transparent; padding: 0; }

/* Images inside a bubble */
.images { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 8px; }
.thumb { width: 100%; height: auto; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-bg); object-fit: contain; cursor: zoom-in; }
.thumb:hover { filter: brightness(1.05); }

/* Meta line (time, badges) styled subtly */
.meta-line { display: flex; align-items: center; gap: 8px; font-size: 11px; opacity: 0.9; }
.row.assistant .meta-line { justify-content: flex-start; color: var(--adc-fg-muted); }
.row.user .meta-line { justify-content: flex-end; color: #e9ebff; }
.badge { background: var(--adc-accent); color: #fff; border-radius: 6px; padding: 1px 6px; font-size: 10px; }

/* Bubble action buttons (copy, read aloud) */
.bubble-actions { position: absolute; top: 6px; right: 6px; display: flex; align-items: center; gap: 6px; opacity: 0; transition: opacity 0.15s ease; z-index: 1; }
.bubble:hover .bubble-actions { opacity: 0.9; }
.bubble-action-btn { width: 24px; height: 24px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--adc-border); background: var(--adc-bg); color: var(--adc-fg); border-radius: 6px; cursor: pointer; padding: 0; font-size: 14px; line-height: 1; }
.bubble-action-btn:hover { filter: brightness(1.05); }
.row.user .bubble-action-btn { background: rgba(255,255,255,0.12); color: #fff; border-color: rgba(255,255,255,0.25); }

/* Tool call rendering */
.tool { display: flex; flex-direction: column; gap: 8px; }
.tool-header { display: flex; align-items: center; gap: 8px; }
.tool-name { font-weight: 600; }
.status { margin-left: auto; font-size: 11px; padding: 2px 6px; border-radius: 6px; background: var(--adc-border); color: var(--adc-fg-muted); }
.status[data-finished="true"][data-ok="true"] { background: #0f9d58; color: #fff; }
.status[data-finished="true"][data-ok="false"] { background: #d93025; color: #fff; }
.section { display: flex; flex-direction: column; gap: 4px; }
.label { font-size: 11px; color: var(--adc-fg-muted); }
.code { padding: 8px; border-radius: 8px; background: #0b0b0b; color: #e8e8e8; border: 1px solid var(--adc-border); white-space: pre-wrap; overflow: auto; max-height: 260px; }
.code.error { background: #290f12; color: #ffd8d8; border-color: #bf3b42; }
</style>
