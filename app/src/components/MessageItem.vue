<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import type { Message } from '../state/conversation'
import 'highlight.js/styles/github-dark.css'

const props = defineProps<{ message: Message; hideToolDetails?: boolean }>()
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
</script>

<template>
  <div class="row" :class="props.message.role">
    <div class="bubble" :data-type="props.message.type">
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
.md-content h1 { font-size: 1.10em; }
.md-content h2 { font-size: 1.06em; }
.md-content h3 { font-size: 1.02em; }
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
