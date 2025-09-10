import { watch } from 'vue'
import type { Ref } from 'vue'

// Import per-style CSS as URL assets; bundler resolves at build time
import sidebarDarkStyleUrl from '../styles/sidebar-dark/style.css?url'
import sidebarLightStyleUrl from '../styles/sidebar-light/style.css?url'

const themeCssLinkId = 'theme-style-css'

const styleCssMap: Record<string, string> = {
  'sidebar-dark': sidebarDarkStyleUrl,
  'sidebar-light': sidebarLightStyleUrl,
}

function ensureThemeLinkEl(): HTMLLinkElement {
  const id = themeCssLinkId
  let el = document.getElementById(id) as HTMLLinkElement | null
  if (!el) {
    el = document.createElement('link')
    el.rel = 'stylesheet'
    el.id = id
    document.head.appendChild(el)
  }
  return el
}

function applyStyleCss(styleName: string) {
  const el = ensureThemeLinkEl()
  const resolved = styleCssMap[String(styleName)]
  if (resolved) {
    el.href = resolved
  } else {
    // Default to dark sidebar layout if unknown value
    el.href = styleCssMap['sidebar-dark']
  }
}

export function useThemeStyle(uiStyle: Ref<string>) {
  // React to changes
  watch(uiStyle, (v) => {
    try { applyStyleCss(v) } catch {}
  })

  return { applyStyleCss }
}
