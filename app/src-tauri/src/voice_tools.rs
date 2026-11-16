// Voice tool profile: which MCP tools a realtime voice session gets, and what
// the app adds to their results.
//
// A realtime voice model chooses tools far worse from a long list than a chat
// model does. With all ~70 windows-commander tools (mouse, keyboard,
// PowerShell, files, registry, ...) plus Claude Code it answered "where is my
// cursor" with a guess and routed "this event" to Claude Code, which cannot
// see the screen. The "screen" profile (default) offers only the tools that
// look at and point at the screen; chat keeps every tool. Measured with
// scripts/voice-eval; see specs/ScreenPresence_design.md, section 5.
//
// The profile applies to the windows-commander server only. Other servers keep
// their tools (minus `disabled_tools`, which applies to every profile).
//
// realtime_call_tool also enforces it: a voice session cannot call a
// windows-commander tool outside the profile even by naming it, since the
// reverse name map is shared with chat and would otherwise resolve it.

use std::time::Duration;

use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;

use crate::mcp::ClientMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
  /// Only the screen tools: look, find, read, point.
  ScreenOnly,
  /// The screen tools plus single-step actions (default): click an element,
  /// set a field, type into the focused field, press a shortcut. Multi-step
  /// work goes to desktop_agent.rs.
  Screen,
  /// Every windows-commander tool, as before.
  All,
}

#[derive(serde::Deserialize)]
struct ProfileFile {
  windows_commander_screen: Vec<String>,
  #[serde(default)]
  windows_commander_actions: Vec<String>,
}

/// The shared lists (also read by the eval), compiled in.
static PROFILE_FILE: Lazy<ProfileFile> = Lazy::new(|| {
  serde_json::from_str::<ProfileFile>(include_str!("../voice_tool_profile.json"))
    .unwrap_or(ProfileFile { windows_commander_screen: Vec::new(), windows_commander_actions: Vec::new() })
});

pub fn screen_tools() -> &'static [String] {
  &PROFILE_FILE.windows_commander_screen
}

pub fn action_tools() -> &'static [String] {
  &PROFILE_FILE.windows_commander_actions
}

/// `"screen"` or anything unknown is the default. `"screen"` meant screen
/// tools only before the actions were added (same day, never released).
pub fn profile_from(settings: &serde_json::Value) -> Profile {
  match settings
    .get("assistant_realtime")
    .and_then(|ar| ar.get("voice_tool_profile"))
    .and_then(|x| x.as_str())
  {
    Some("all") => Profile::All,
    Some("screen_only") => Profile::ScreenOnly,
    _ => Profile::Screen,
  }
}

pub fn profile_from_settings() -> Profile {
  profile_from(&crate::config::load_settings_json())
}

/// Whether a voice session may see / call `fn_name` (a mangled
/// `mcp__server__tool` name). Tools of other servers are always allowed here;
/// `disabled_tools` is applied elsewhere.
pub fn allowed(fn_name: &str, wc_server_id: &str, profile: Profile) -> bool {
  if profile == Profile::All { return true; }
  let prefix = format!("mcp__{}__", crate::mcp::sanitize_fn_component(wc_server_id));
  if !fn_name.starts_with(&prefix) { return true; }
  let is = |list: &[String]| list.iter().any(|t| crate::mcp::mangled_tool_name(wc_server_id, t) == fn_name);
  is(screen_tools()) || (profile == Profile::Screen && is(action_tools()))
}

/// Keep the flat realtime tool definitions the profile allows.
pub fn filter_tools(tools: Vec<serde_json::Value>, wc_server_id: &str, profile: Profile) -> Vec<serde_json::Value> {
  tools
    .into_iter()
    .filter(|t| t.get("name").and_then(|n| n.as_str()).map(|n| allowed(n, wc_server_id, profile)).unwrap_or(false))
    .collect()
}

// ---------------------------------------------------------------------------
// get_pointer_context, made answerable
// ---------------------------------------------------------------------------

/// "On screen 2: horizontally left of centre (38 %), vertically below the
/// middle (63 %)." Voice models are poor at turning pixel coordinates into
/// "where", and guessed instead; this gives them the sentence.
pub fn describe_position(pointer: &serde_json::Value) -> Option<String> {
  let m = pointer.get("monitor")?;
  let b = m.get("bounds")?;
  let (bx, by) = (b.get("x")?.as_f64()?, b.get("y")?.as_f64()?);
  let (bw, bh) = (b.get("width")?.as_f64()?, b.get("height")?.as_f64()?);
  if bw <= 0.0 || bh <= 0.0 { return None; }
  let (x, y) = (pointer.get("x")?.as_f64()?, pointer.get("y")?.as_f64()?);
  let fx = (x - bx) / bw;
  let fy = (y - by) / bh;
  let h = if fx < 0.2 { "near the left edge" } else if fx < 0.4 { "left of centre" } else if fx <= 0.6 { "in the centre" } else if fx < 0.8 { "right of centre" } else { "near the right edge" };
  let v = if fy < 0.2 { "near the top" } else if fy < 0.4 { "above the middle" } else if fy <= 0.6 { "in the middle" } else if fy < 0.8 { "below the middle" } else { "near the bottom" };
  let screen = m
    .get("monitorId")
    .and_then(|s| s.as_str())
    .and_then(|s| {
      let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
      if digits.is_empty() { None } else { Some(format!("screen {digits}")) }
    })
    .unwrap_or_else(|| "the screen".to_string());
  Some(format!(
    "On {screen}: horizontally {h} ({} %), vertically {v} ({} %).",
    (fx * 100.0).round() as i64,
    (fy * 100.0).round() as i64
  ))
}

/// True when the element under the cursor says nothing beyond the window: a
/// Window or Pane with no name, or with the window's own title. That is what
/// UI Automation hit testing returns for apps whose content is a WebView2 in
/// visual hosting mode, the new Outlook among them.
pub fn element_is_shallow(pointer: &serde_json::Value) -> bool {
  let Some(el) = pointer.get("element").filter(|e| e.is_object()) else { return false; };
  let ctype = el.get("controlType").and_then(|x| x.as_str()).unwrap_or("");
  if ctype != "Window" && ctype != "Pane" { return false; }
  let name = el.get("name").and_then(|x| x.as_str()).unwrap_or("").trim();
  let title = pointer.get("window").and_then(|w| w.get("title")).and_then(|x| x.as_str()).unwrap_or("").trim();
  name.is_empty() || name == title
}

/// The smallest named element in `elements` (a read_ui_tree list) whose
/// bounds contain (x, y).
pub fn element_at(elements: &[serde_json::Value], x: f64, y: f64) -> Option<serde_json::Value> {
  let mut best: Option<(f64, &serde_json::Value)> = None;
  for e in elements {
    let name = e.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
    if name.is_empty() { continue; }
    let Some(b) = e.get("bounds") else { continue; };
    let (Some(bx), Some(by), Some(bw), Some(bh)) = (
      b.get("x").and_then(|v| v.as_f64()), b.get("y").and_then(|v| v.as_f64()),
      b.get("width").and_then(|v| v.as_f64()), b.get("height").and_then(|v| v.as_f64()),
    ) else { continue; };
    if bw <= 0.0 || bh <= 0.0 || x < bx || x >= bx + bw || y < by || y >= by + bh { continue; }
    let area = bw * bh;
    if best.map(|(a, _)| area < a).unwrap_or(true) { best = Some((area, e)); }
  }
  best.map(|(_, e)| serde_json::json!({
    "elementRef": e.get("elementRef").cloned().unwrap_or(serde_json::Value::Null),
    "name": e.get("name").cloned().unwrap_or(serde_json::Value::Null),
    "controlType": e.get("controlType").cloned().unwrap_or(serde_json::Value::Null),
    "bounds": e.get("bounds").cloned().unwrap_or(serde_json::Value::Null),
  }))
}

/// Control types worth naming under a cursor. Keeps the tree read small (the
/// Outlook calendar has about 470 elements, these are about 230 of them) and
/// leaves out Text, whose fragments are smaller than the item they belong to
/// and would win the "smallest element" pick with half a name.
const POINTABLE: &[&str] = &["Button", "ListItem", "DataItem", "TreeItem", "Hyperlink", "MenuItem", "TabItem", "CheckBox", "Edit"];

async fn call_json(clients: &AsyncMutex<ClientMap>, server: &str, tool: &str, args: serde_json::Value) -> Option<serde_json::Value> {
  let res = crate::mcp::call_tool(clients, server, tool, args).await.ok()?;
  crate::screen_layer::tool_result_json(&res)
}

/// Look up what is really under the cursor in a WebView2 app: its content
/// lives in a separate top-level `msedgewebview2` window with the same title,
/// whose UIA tree has the elements the host window's hit test does not.
async fn resolve_webview_element(clients: &AsyncMutex<ClientMap>, server: &str, pointer: &serde_json::Value) -> Option<serde_json::Value> {
  let title = pointer.get("window")?.get("title")?.as_str()?.trim().to_string();
  if title.is_empty() { return None; }
  let (x, y) = (pointer.get("x")?.as_f64()?, pointer.get("y")?.as_f64()?);
  let found = call_json(clients, server, "find_window", serde_json::json!({
    "process_name": "msedgewebview2", "process_name_exact": true, "title_contains": title, "visible_only": true,
  })).await?;
  let hwnd = found.as_array()?.iter().find(|w| w.get("title").and_then(|t| t.as_str()).map(str::trim) == Some(title.as_str()))?.get("hwnd")?.as_i64()?;
  let tree = call_json(clients, server, "read_ui_tree", serde_json::json!({
    "hwnd": hwnd, "max_depth": 20, "control_types": POINTABLE,
  })).await?;
  let mut el = element_at(tree.get("elements")?.as_array()?, x, y)?;
  if let Some(o) = el.as_object_mut() {
    o.insert("source".into(), serde_json::json!("UI Automation tree of the app's WebView2 content window"));
  }
  Some(el)
}

/// Add `position` and, when the hit test stopped at the window, a
/// `resolved_element` to a get_pointer_context result (the MCP result object,
/// changed in place). Blocked windows are left alone.
pub async fn enrich_pointer_result(clients: &AsyncMutex<ClientMap>, server: &str, result: &mut serde_json::Value) {
  let Some(mut pointer) = crate::screen_layer::tool_result_json(result) else { return; };
  if pointer.get("blocked").and_then(|b| b.as_bool()) == Some(true) { return; }
  let Some(obj) = pointer.as_object_mut() else { return; };
  if let Some(pos) = describe_position(&serde_json::Value::Object(obj.clone())) {
    obj.insert("position".into(), serde_json::json!(pos));
  }
  let snapshot = serde_json::Value::Object(obj.clone());
  if element_is_shallow(&snapshot) {
    match tokio::time::timeout(Duration::from_secs(5), resolve_webview_element(clients, server, &snapshot)).await {
      Ok(Some(el)) => { obj.insert("resolved_element".into(), el); }
      Ok(None) => {}
      Err(_) => log::debug!("voice tools: WebView2 element lookup timed out"),
    }
  }
  set_first_text_json(result, &pointer);
}

/// Replace the first text block of an MCP result with `value` as JSON.
fn set_first_text_json(result: &mut serde_json::Value, value: &serde_json::Value) {
  let Some(items) = result.get_mut("content").and_then(|c| c.as_array_mut()) else { return; };
  for it in items.iter_mut() {
    if it.get("type").and_then(|t| t.as_str()) == Some("text") {
      if let Some(o) = it.as_object_mut() { o.insert("text".into(), serde_json::json!(value.to_string())); }
      return;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn profile_file_parses_and_has_the_pointer_tools() {
    let t = screen_tools();
    assert!(t.iter().any(|x| x == "get_pointer_context"));
    assert!(t.iter().any(|x| x == "point_at"));
    assert!(!t.iter().any(|x| x == "execute_powershell"));
  }

  #[test]
  fn profile_defaults_to_screen() {
    assert_eq!(profile_from(&json!({})), Profile::Screen);
    assert_eq!(profile_from(&json!({ "assistant_realtime": { "voice_tool_profile": "all" } })), Profile::All);
    assert_eq!(profile_from(&json!({ "assistant_realtime": { "voice_tool_profile": "bogus" } })), Profile::Screen);
    assert_eq!(profile_from(&json!({ "assistant_realtime": { "voice_tool_profile": "screen_only" } })), Profile::ScreenOnly);
  }

  #[test]
  fn actions_only_in_the_default_profile() {
    assert!(action_tools().iter().any(|x| x == "invoke_ui_element"));
    let click = "mcp__windows-commander__invoke_ui_element";
    assert!(allowed(click, "windows-commander", Profile::Screen));
    assert!(!allowed(click, "windows-commander", Profile::ScreenOnly));
    assert!(allowed("mcp__windows-commander__point_at", "windows-commander", Profile::ScreenOnly));
    // Mouse, PowerShell and files stay with chat and the desktop agent.
    assert!(!allowed("mcp__windows-commander__mouse_action", "windows-commander", Profile::Screen));
  }

  #[test]
  fn screen_profile_keeps_other_servers_and_drops_the_rest_of_windows_commander() {
    let tools = vec![
      json!({ "name": "mcp__windows-commander__point_at" }),
      json!({ "name": "mcp__windows-commander__execute_powershell" }),
      json!({ "name": "mcp__windows-commander__mouse_action" }),
      json!({ "name": "mcp__Rob__ask_claude" }),
      json!({ "name": "desktop_agent" }),
    ];
    let kept: Vec<String> = filter_tools(tools.clone(), "windows-commander", Profile::Screen)
      .iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(kept, vec!["mcp__windows-commander__point_at", "mcp__Rob__ask_claude", "desktop_agent"]);
    assert_eq!(filter_tools(tools, "windows-commander", Profile::All).len(), 5);
    assert!(!allowed("mcp__windows-commander__read_registry", "windows-commander", Profile::Screen));
  }

  #[test]
  fn describes_the_measured_cursor_position() {
    // The call on 11.11.2026: x 2889, y 911 on the 2560 x 1440 second screen.
    let p = json!({ "x": 2889, "y": 911, "monitor": { "monitorId": "screen-2", "bounds": { "x": 1920, "y": 0, "width": 2560, "height": 1440 } } });
    assert_eq!(describe_position(&p).unwrap(), "On screen 2: horizontally left of centre (38 %), vertically below the middle (63 %).");
    let edge = json!({ "x": 4210, "y": 180, "monitor": { "monitorId": "screen-2", "bounds": { "x": 1920, "y": 0, "width": 2560, "height": 1440 } } });
    assert!(describe_position(&edge).unwrap().contains("near the right edge"));
    assert!(describe_position(&json!({ "x": 1, "y": 1 })).is_none());
  }

  #[test]
  fn window_hit_is_shallow_and_a_named_button_is_not() {
    let olk = json!({ "window": { "title": "Calendar - Outlook" }, "element": { "controlType": "Window", "name": "Calendar - Outlook" } });
    assert!(element_is_shallow(&olk));
    let pane = json!({ "window": { "title": "x" }, "element": { "controlType": "Pane", "name": "" } });
    assert!(element_is_shallow(&pane));
    let button = json!({ "window": { "title": "x" }, "element": { "controlType": "Button", "name": "Send" } });
    assert!(!element_is_shallow(&button));
  }

  #[test]
  fn picks_the_smallest_named_element_under_the_point() {
    let els = vec![
      json!({ "elementRef": "g", "name": "calendar view", "controlType": "Group", "bounds": { "x": 2252, "y": 239, "width": 2221, "height": 1154 } }),
      json!({ "elementRef": "anon", "name": "", "controlType": "Group", "bounds": { "x": 2700, "y": 874, "width": 448, "height": 76 } }),
      json!({ "elementRef": "ev", "name": "Steering Committee, 14:00 to 15:00", "controlType": "Button", "bounds": { "x": 2700, "y": 874, "width": 448, "height": 76 } }),
      json!({ "elementRef": "other", "name": "Lunch", "controlType": "Button", "bounds": { "x": 2303, "y": 674, "width": 184, "height": 75 } }),
    ];
    let hit = element_at(&els, 2961.0, 904.0).unwrap();
    assert_eq!(hit["elementRef"], "ev");
    assert!(element_at(&els, 10.0, 10.0).is_none());
  }

  #[test]
  fn rewrites_the_text_block() {
    let mut res = json!({ "content": [{ "type": "text", "text": "{\"x\":1}" }] });
    set_first_text_json(&mut res, &json!({ "x": 1, "position": "p" }));
    assert_eq!(crate::screen_layer::tool_result_json(&res).unwrap()["position"], "p");
  }
}
