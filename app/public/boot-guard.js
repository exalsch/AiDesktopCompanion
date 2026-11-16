// Reload the page when one of its own scripts or stylesheets fails to load.
//
// After hibernation the dev build came back as a white window: Vite's client
// reloaded the page when it reconnected, the network was still changing, and
// main.ts's imports failed with net::ERR_NETWORK_CHANGED (15.11.2026). One
// failed module stops the whole module graph, so Vue never mounts, and no
// WebView2 process died, so webview_health.rs had nothing to react to.
//
// A classic script, not a module, so it runs even when the module graph does
// not; a separate file, because the CSP allows no inline script. The release
// build loads its files from the app itself, so this should never fire there.
//
// Retries never stop, they slow down: three quick ones 3 s apart, then 15 s,
// 30 s, 60 s and so on up to 5 minutes. An earlier version gave up for good
// after three reloads in a minute, and a page that dies has no script left to
// try again - so a network that kept changing for more than about ten seconds
// left the window dead until someone refreshed it by hand (16.11.2026). A page
// that loads cleanly clears the count; so does a pause of ten minutes.
(function () {
  var KEY = 'boot-guard-retries'
  var QUICK = 3
  var QUICK_MS = 3000
  var SLOW_MS = 15000
  var MAX_MS = 300000
  var STALE_MS = 600000
  var scheduled = false
  var failed = false

  function read() {
    try {
      var s = JSON.parse(sessionStorage.getItem(KEY) || 'null')
      if (s && typeof s.n === 'number' && Date.now() - s.t < STALE_MS) return s
    } catch (e) {}
    return { n: 0, t: 0 }
  }

  function write(s) {
    try { sessionStorage.setItem(KEY, JSON.stringify(s)) } catch (e) {}
  }

  /** Wait before reload number `n` (1-based) of a run of failed loads. */
  function delayFor(n) {
    if (n <= QUICK) return QUICK_MS
    return Math.min(MAX_MS, SLOW_MS * Math.pow(2, n - QUICK - 1))
  }

  function reloadSoon(what) {
    failed = true
    if (scheduled) return
    scheduled = true
    var s = read()
    s.n += 1
    s.t = Date.now()
    write(s)
    var ms = delayFor(s.n)
    console.warn('[boot-guard] ' + what + ' failed to load; reload ' + s.n + ' in ' + Math.round(ms / 1000) + ' s')
    setTimeout(function () { location.reload() }, ms)
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

  // `load` waits for the module scripts, so a failed import has reported by
  // now. A clean load ends the run of retries.
  window.addEventListener('load', function () {
    if (failed) return
    try { sessionStorage.removeItem(KEY) } catch (e) {}
  })
})()
