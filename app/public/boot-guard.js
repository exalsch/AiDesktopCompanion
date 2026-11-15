// Reload the page when one of its own scripts or stylesheets fails to load.
//
// After hibernation the dev build came back as a white window: Vite's client
// reloaded the page when it reconnected, the network was still changing, and
// main.ts's imports failed with net::ERR_NETWORK_CHANGED (15.11.2026). One
// failed module stops the whole module graph, so Vue never mounts, and no
// WebView2 process died, so webview_health.rs had nothing to react to.
//
// A classic script, not a module, so it runs even when the module graph does
// not; a separate file, because the CSP allows no inline script. At most three
// reloads a minute per window, so a server that is really gone does not cause
// a reload loop. The release build loads its files from the app itself, so
// this should never fire there.
(function () {
  var KEY = 'boot-guard-reloads'
  var WINDOW_MS = 60000
  var MAX = 3
  var scheduled = false

  function recent() {
    try {
      var now = Date.now()
      return JSON.parse(sessionStorage.getItem(KEY) || '[]').filter(function (t) { return now - t < WINDOW_MS })
    } catch (e) { return [] }
  }

  function reloadSoon(what) {
    if (scheduled) return
    var times = recent()
    if (times.length >= MAX) {
      console.warn('[boot-guard] ' + what + ' failed to load; not reloading, ' + MAX + ' reloads in the last minute')
      return
    }
    scheduled = true
    console.warn('[boot-guard] ' + what + ' failed to load; reloading in 3 s')
    try { times.push(Date.now()); sessionStorage.setItem(KEY, JSON.stringify(times)) } catch (e) {}
    setTimeout(function () { location.reload() }, 3000)
  }

  // Resource errors do not bubble, so listen in the capture phase.
  window.addEventListener('error', function (e) {
    var t = e && e.target
    if (!t || t === window) return
    var tag = t.tagName
    if (tag === 'SCRIPT' || (tag === 'LINK' && /stylesheet|modulepreload/.test(t.rel || ''))) {
      reloadSoon(t.src || t.href || tag)
    }
  }, true)
})()
