# UI Styles

This app supports multiple UI styles (layouts) with their own CSS files. The current built‑in styles are:

- sidebar: Sidebar navigation layout (default)
- tabs: Top tabs layout (legacy)

The active style is stored in `settings.ui_style` and can be switched in the Settings tab of the main window.

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
      sidebar/
        style.css
      tabs/
        style.css
```

‼️ The provided style files are placeholders — customize them to your needs.

## Add a new style

1) Create the CSS file
- Path: `app/src/styles/<styleName>/style.css`
- Start by copying from an existing style and adjust overrides.

2) Import and map the CSS in `App.vue`
- Add an import at the top:
```ts
import myStyleUrl from './styles/myStyle/style.css?url'
```
- Extend the style map in the loader section:
```ts
const styleCssMap: Record<'sidebar' | 'tabs' | 'myStyle', string> = {
  sidebar: sidebarStyleUrl,
  tabs: tabsStyleUrl,
  myStyle: myStyleUrl,
}
```

3) Make the style selectable (optional)
- Update the UI Style dropdown in `App.vue` Settings section to include an `<option value="myStyle">My Style</option>`.
- Ensure any type definitions reflect the new literal (TypeScript unions).

4) Test
- With the dev server running, switch styles in Settings and verify HMR applies the new CSS without page reload.

## Notes & tips

- Cascade order: style CSS loads after base `style.css`, so it can override base rules.
- Scope: the dynamic loader runs only in the main window; enable it in other windows if you want consistent theming.
- CSP: the CSS is bundled and referenced by URL; current CSP allows local styles.
- Troubleshooting: if the CSS doesn’t apply, check the presence of `<link id="theme-style-css">` in DevTools and verify the mapped URL.
