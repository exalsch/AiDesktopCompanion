import { watch } from 'vue'
import type { Ref } from 'vue'

/**
 * Applies the CSS for the selected `ui_style` to the document.
 *
 * The stylesheets are imported with `?inline`, which hands us the CSS as a
 * string and lets us put it in a single <style> element we own.
 *
 * The alternative - `?url` on a <link> - looks tidier but has two problems:
 * the browser caches that URL, so editing a theme during `tauri dev` shows
 * nothing until the whole app is restarted; and switching styles at runtime
 * fetches a file, which flashes the old colours until it lands. Inlining costs
 * about a kilobyte per theme in the bundle and avoids both.
 *
 * Inline styles are permitted: `style-src` in tauri.conf.json includes
 * 'unsafe-inline' for both csp and devCsp.
 */
import sidebarDarkCss from '../styles/sidebar-dark/style.css?inline'
import sidebarLightCss from '../styles/sidebar-light/style.css?inline'

const themeStyleElId = 'theme-style-css'

const styleCssMap: Record<string, string> = {
  'sidebar-dark': sidebarDarkCss,
  'sidebar-light': sidebarLightCss,
}

const DEFAULT_STYLE = 'sidebar-dark'

function ensureThemeStyleEl(): HTMLStyleElement {
  let el = document.getElementById(themeStyleElId) as HTMLElement | null
  // This id used to belong to a <link>. Setting textContent on one of those
  // does nothing at all - it would fail silently and leave the old theme
  // applied - so drop anything that is not the <style> we expect.
  if (el && el.tagName !== 'STYLE') {
    el.remove()
    el = null
  }
  if (!el) {
    el = document.createElement('style')
    el.id = themeStyleElId
    // Appended last so its :root token overrides win over src/style.css
    // without needing !important anywhere.
    document.head.appendChild(el)
  }
  return el as HTMLStyleElement
}

function applyStyleCss(styleName: string) {
  const el = ensureThemeStyleEl()
  const css = styleCssMap[String(styleName)] ?? styleCssMap[DEFAULT_STYLE]
  if (el.textContent !== css) el.textContent = css
}

export function useThemeStyle(uiStyle: Ref<string>) {
  watch(uiStyle, (v) => {
    try { applyStyleCss(v) } catch {}
  }, { immediate: true })

  return { applyStyleCss }
}
