/**
 * One MarkdownIt + DOMPurify + highlight.js for the whole app.
 *
 * Shared rather than per-component so there is a single sanitiser policy: chat
 * messages and the What's New dialog must not be able to drift into different
 * ideas of what HTML is safe. The imports are dynamic so the vendor chunks are
 * only fetched once something actually renders markdown.
 */

let _mdReady: Promise<(src: string) => string> | null = null

function _escapeHtml(s: string): string {
  return (s ?? '')
    .replaceAll(/&/g, '&amp;')
    .replaceAll(/</g, '&lt;')
    .replaceAll(/>/g, '&gt;')
}

/** Minimal renderer for when the dynamic imports fail. */
function _basicFallback(input: string): string {
  let s = (input ?? '').replace(/\r\n/g, '\n')
  s = _escapeHtml(s)
  return s.split(/\n\n+/).map(p => `<p>${p.replace(/\n/g, '<br/>')}</p>`).join('')
}

export function initMarkdownSingleton(): Promise<(src: string) => string> {
  if (_mdReady) return _mdReady
  _mdReady = (async () => {
    try {
      const [{ default: MarkdownIt }, { default: DOMPurify }] = await Promise.all([
        import('markdown-it'),
        import('dompurify'),
      ])

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
          return `<pre class="md-pre"><code>${_escapeHtml(code)}</code></pre>`
        },
      })
      return (src: string) => DOMPurify.sanitize(md.render(src))
    } catch {
      // Cleared so a later call can retry after a transient import failure.
      _mdReady = null
      return (src: string) => _basicFallback(src)
    }
  })()
  return _mdReady as Promise<(src: string) => string>
}
