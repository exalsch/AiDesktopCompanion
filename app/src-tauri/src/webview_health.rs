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
//! So this module only watches and reports. It changes no behaviour: it attaches
//! a `ProcessFailed` handler to each long-lived window and logs which kind of
//! failure occurred. GPU_PROCESS_EXITED and BROWSER_PROCESS_EXITED want opposite
//! repairs - a nudge versus a recreate - so knowing which one happened is the
//! whole point. The next occurrence should say.
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

/// Attach a `ProcessFailed` handler to one window's WebView2.
///
/// Failures here are logged and swallowed: this is instrumentation, and a
/// diagnostic that can stop a window from opening is worse than the bug it was
/// added to investigate.
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
    let handler = ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
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

      let (name, note) = match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => (
          "GPU_PROCESS_EXITED",
          " - the suspected hibernation case; the WebView is alive and a nudge should repair it",
        ),
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => (
          "BROWSER_PROCESS_EXITED",
          " - the WebView is gone; only recreating it will help",
        ),
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => ("RENDER_PROCESS_EXITED", " - a reload should repair it"),
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => ("RENDER_PROCESS_UNRESPONSIVE", ""),
        other => {
          println!("[webview-health] {for_event}: PROCESS FAILED kind={} (unrecognised)", other.0);
          return Ok(());
        }
      };
      println!("[webview-health] {for_event}: PROCESS FAILED {name}{note}");
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
