# UI Styles

This app supports multiple UI styles (layouts) with their own CSS files. The supported built‑in styles are:

- sidebar-dark: Sidebar navigation layout (default, dark)
- sidebar-light: Sidebar navigation layout (light)

The active style is stored in `settings.ui_style` and can be switched in the Settings tab of the main window.

Back‑compat note:
- Legacy values `sidebar` and `light` are still accepted and automatically mapped to `sidebar-dark` and `sidebar-light` respectively. Any legacy `tabs` value is coerced to `sidebar-dark`.

## How it works

- Base styles are in `app/src/style.css` and apply to all windows.
- Style‑specific CSS is loaded dynamically in `app/src/App.vue` by injecting a `<link id="theme-style-css">` tag into `<head>`.
- For the main window only, `App.vue` maps the current style to its bundled CSS asset and sets the link’s `href` accordingly.
- Quick Actions and Capture Overlay windows intentionally skip style CSS for a minimal, consistent UI.

Relevant code:
- `app/src/App.vue` — see the "Per-style CSS loader" section with `applyStyleCss()` and `styleCssMap`.
- `app/src/main.ts` — imports the base `style.css`.

## Directory structure

```
app/
  src/
    styles/
      sidebar-dark/
        style.css
      sidebar-light/
        style.css
      (legacy, not used by loader):
        sidebar/
          style.css
        light/
          style.css
        tabs/
          style.css
```

‼️ The provided style files are placeholders — customize them to your needs.

## Theme tokens and inputs

- Base tokens are defined in `app/src/style.css` under `:root` for the default dark theme.
- The Sidebar Light theme overrides tokens in `app/src/styles/sidebar-light/style.css`:
  - `--adc-bg`, `--adc-surface`, `--adc-fg`, `--adc-fg-muted`, `--adc-border`, `--adc-accent`, etc.

### Inputs and Textareas

- Use the shared `.input` class on `<input>`, `<select>`, and `<textarea>` elements.
- Avoid hard‑coded colors in component‑scoped CSS. Instead, use tokens:
  - Background: `background: var(--adc-surface)`
  - Foreground: `color: var(--adc-fg)`
  - Border: `border: 1px solid var(--adc-border)`
  - Focus: `box-shadow: 0 0 0 3px var(--adc-focus-ring); border-color: var(--adc-accent)`
- Add `box-sizing: border-box` for textareas to prevent overflow.
- Example (scoped):

```css
.label { color: var(--adc-fg-muted); }
.input { padding: 8px 10px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); }
textarea { width: 100%; min-height: 100px; resize: vertical; box-sizing: border-box; }
```

### Responsive grids

- Prefer `grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));` to avoid overflow in editors like Quick Prompts.
- Ensure children have `min-width: 0` to allow shrinking inside grid cells.

## Add a new style

1) Create the CSS file
- Path: `app/src/styles/<styleName>/style.css`
- Start by copying from an existing style and adjust overrides.

2) Import and map the CSS in `App.vue`
- Add an import at the top:
```ts
import myStyleUrl from './styles/myStyle/style.css?url'
```
- Extend the style map in the loader section (string map recommended):
```ts
const styleCssMap: Record<string, string> = {
  'sidebar-dark': sidebarDarkStyleUrl,
  'sidebar-light': sidebarLightStyleUrl,
  'myStyle': myStyleUrl,
  // Back-compat aliases (optional):
  'sidebar': sidebarDarkStyleUrl,
  'light': sidebarLightStyleUrl,
}
```

3) Make the style selectable (optional)
- Update the UI Style dropdown in `App.vue` Settings section to include an `<option value="myStyle">My Style</option>`.
- Ensure any type definitions reflect the new literal (TypeScript unions). Current union is `'sidebar-dark' | 'sidebar-light'`.

4) Test
- With the dev server running, switch styles in Settings and verify HMR applies the new CSS without page reload.

## Persistence

- The active style is stored in `settings.ui_style` and loaded on startup in `App.vue` (`loadSettings()`).
- The CSS for the selected style is applied via `applyStyleCss()` on mount and whenever the setting changes.
- To persist a change, click “Save Settings”. ‼️ If you prefer auto‑save on style change, consider adding a watcher that invokes `save_settings` for `ui_style` only.

## Notes & tips

- Cascade order: style CSS loads after base `style.css`, so it can override base rules.
- Scope: the dynamic loader runs only in the main window; enable it in other windows if you want consistent theming.
- CSP: the CSS is bundled and referenced by URL; current CSP allows local styles.
- Troubleshooting: if the CSS doesn’t apply, check the presence of `<link id="theme-style-css">` in DevTools and verify the mapped URL.
