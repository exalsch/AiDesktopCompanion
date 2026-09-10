//! Diagnosis for the blank-window-after-hibernate bug (issue #14).
//!
//! Symptom: after resuming from hibernation the Quick Actions popup appears as a
//! blank white window that never repaints. The hotkey fires and the window is
//! shown, so the failure is in the WebView content, not the shortcut.
//!
//! The suspected mechanism is WebView2's GPU process dying with exit code 34,
//! "the GPU process was terminated due to context lost", which Microsoft ties to
//! hibernation and describes as *supposed* to auto-recover
//! (WebView2Feedback#3817). But nothing in the app can currently tell that apart
//! from a second possibility - a window that thinks it is visible while its
//! surface is gone - and the two want different repairs. One is recoverable by
//! nudging the existing WebView; the other means the browser process is gone and
//! only a reload or recreate will do.
//!
//! The occurrence came, and it named RENDER_PROCESS_EXITED. A display topology
//! change - unplugging two of three monitors - kills the render processes of
//! every long-lived window at once. The windows survive: they are still shown,
//! still positioned correctly, still `IsWindowVisible`. Only their content is
//! gone. That is invisible on the two transparent pill windows, and it takes the
//! main window's JS with it, which is what registers the global hotkeys and what
//! drives both pills - so the hotkeys go dead and no pill can ever appear again
//! until the app is restarted.
//!
//! So this module now repairs as well as reports. Each failure kind gets the one
//! repair that suits it:
//!   * RENDER_PROCESS_EXITED - reload; this is the recoverable case and the one
//!     seen in the wild. Microsoft documents `Reload` as the recovery here.
//!   * GPU_PROCESS_EXITED - log only. WebView2 re-creates the GPU process by
//!     itself and the content keeps painting, so a reload would throw away good
//!     state (a live Assistant call included) to fix nothing.
//!   * BROWSER_PROCESS_EXITED - log only. The WebView2 environment is gone and
//!     a reload has nothing left to talk to; only recreating the window helps,
//!     which is more than this module should do on its own.
//!
//! Tauri already does the equivalent recovery on macOS - `tauri-runtime-wry`
//! installs a default `on_web_content_process_terminate` handler that reloads the
//! webview - but it is `#[cfg(macos, ios)]` with no WebView2 equivalent, which is
//! why this lives in app code.

/// Windows that exist for the life of the process and are only ever shown and
/// hidden, never recreated. They are the ones exposed to this: a window that is
/// rebuilt gets a fresh WebView and repairs itself by accident.
pub const LONG_LIVED_WINDOWS: [&str; 4] = ["main", "quick-actions", "busy-indicator", "assistant-pill"];

#[cfg(target_os = "windows")]
pub fn watch_all(app: &tauri::AppHandle) {
  use tauri::Manager;

  for label in LONG_LIVED_WINDOWS {
    match app.get_webview_window(label) {
      Some(win) => watch(&win),
      None => println!("[webview-health] no window labelled '{label}' to watch"),
    }
  }
}

#[cfg(not(target_os = "windows"))]
pub fn watch_all(_app: &tauri::AppHandle) {}

/// How many reloads one window may be given inside `RELOAD_WINDOW`.
///
/// A render process that dies once is an accident worth repairing. One that dies
/// again the moment it comes back is a crash loop, and reloading into it forever
/// would burn the CPU and hide the real fault. After the budget is spent the
/// failure is logged and left alone.
#[cfg(target_os = "windows")]
const RELOAD_BUDGET: usize = 3;

#[cfg(target_os = "windows")]
const RELOAD_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

/// Recent reload times per window label, for the budget above.
#[cfg(target_os = "windows")]
static RELOADS: once_cell::sync::Lazy<
  std::sync::Mutex<std::collections::HashMap<String, Vec<std::time::Instant>>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Whether `label` may be reloaded now, recording the attempt when it may.
///
/// A poisoned lock allows the reload: the repair matters more than the counter.
#[cfg(target_os = "windows")]
fn may_reload(label: &str) -> bool {
  let Ok(mut map) = RELOADS.lock() else { return true };
  let now = std::time::Instant::now();
  let seen = map.entry(label.to_string()).or_default();
  seen.retain(|t| now.duration_since(*t) < RELOAD_WINDOW);
  if seen.len() >= RELOAD_BUDGET {
    return false;
  }
  seen.push(now);
  true
}

/// Attach a `ProcessFailed` handler to one window's WebView2.
///
/// Failures here are logged and swallowed: a handler that can stop a window from
/// opening is worse than the bug it was added to repair.
#[cfg(target_os = "windows")]
fn watch(window: &tauri::WebviewWindow) {
  let label = window.label().to_string();
  let label_for_closure = label.clone();

  let attached = window.with_webview(move |webview| {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
      COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
      COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
      COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
      COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };
    use webview2_com::ProcessFailedEventHandler;

    let controller = webview.controller();
    let core = match unsafe { controller.CoreWebView2() } {
      Ok(c) => c,
      Err(e) => {
        println!("[webview-health] {label_for_closure}: could not reach CoreWebView2: {e}");
        return;
      }
    };

    let for_event = label_for_closure.clone();
    let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
      let Some(args) = args else {
        println!("[webview-health] {for_event}: process failed, no details supplied");
        return Ok(());
      };

      // The exit code and process description live on a later interface that
      // binds to a different `windows` version than this crate uses, so reaching
      // them means pinning a second copy in lockstep. The kind alone answers the
      // question that decides the repair - whether the browser process is gone,
      // or only the GPU process is - so it is not worth the version gymnastics.
      let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
      if let Err(e) = unsafe { args.ProcessFailedKind(&mut kind) } {
        println!("[webview-health] {for_event}: process failed, kind unreadable: {e}");
        return Ok(());
      }

      // `reload` is the whole decision: only the render process leaves behind a
      // WebView that can be talked to and a page worth putting back.
      let (name, note, reload) = match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => (
          "GPU_PROCESS_EXITED",
          " - WebView2 restarts the GPU process itself and the content keeps painting; left alone",
          false,
        ),
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => (
          "BROWSER_PROCESS_EXITED",
          " - the WebView is gone; only recreating it will help",
          false,
        ),
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => {
          ("RENDER_PROCESS_EXITED", " - reloading", true)
        }
        // Unresponsive is not dead. It arrives repeatedly while a page is merely
        // slow, and reloading one would throw away work the user is waiting on.
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
          ("RENDER_PROCESS_UNRESPONSIVE", " - left alone; it may still recover", false)
        }
        other => {
          println!("[webview-health] {for_event}: PROCESS FAILED kind={} (unrecognised)", other.0);
          return Ok(());
        }
      };
      println!("[webview-health] {for_event}: PROCESS FAILED {name}{note}");

      if reload {
        let Some(sender) = sender else {
          println!("[webview-health] {for_event}: no WebView to reload");
          return Ok(());
        };
        if !may_reload(&for_event) {
          println!(
            "[webview-health] {for_event}: not reloading, {RELOAD_BUDGET} reloads already used in the last {}s",
            RELOAD_WINDOW.as_secs()
          );
          return Ok(());
        }
        match unsafe { sender.Reload() } {
          Ok(()) => println!("[webview-health] {for_event}: reloaded"),
          Err(e) => println!("[webview-health] {for_event}: reload failed: {e}"),
        }
      }
      Ok(())
    }));

    let mut token = 0i64;
    match unsafe { core.add_ProcessFailed(&handler, &mut token) } {
      Ok(()) => println!("[webview-health] watching '{label_for_closure}'"),
      Err(e) => println!("[webview-health] {label_for_closure}: could not subscribe to ProcessFailed: {e}"),
    }
  });

  if let Err(e) = attached {
    println!("[webview-health] {label}: with_webview failed: {e}");
  }
}
