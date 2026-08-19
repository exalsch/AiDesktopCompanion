use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::CONTENT_TYPE;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use reqwest::Client;
use serde_json;
use uuid::Uuid;
use futures_util::{StreamExt, TryStreamExt};
use std::convert::Infallible;
use std::time::{Duration, Instant};

/// hyper 1.x has no built-in `Body`; every response has to name its own body
/// type. This server returns either a short in-memory message or a streamed
/// upstream response, so they are erased behind one boxed body.
type ResponseBody = BoxBody<Bytes, std::io::Error>;

/// A complete, in-memory body - the error branches and the 404.
fn fixed_body(message: impl Into<Bytes>) -> ResponseBody {
    Full::new(message.into()).map_err(|never| match never {}).boxed()
}

#[derive(Clone)]
pub struct StreamingSession {
    pub text: String,
    pub voice: String,
    pub model: String,
    pub format: String,
    pub api_key: String,
    pub instructions: Option<String>,
    pub cancel: Arc<AtomicBool>,
    pub created_at: Instant,
    pub started: Arc<AtomicBool>,
}

pub struct TtsStreamingServer {
    port: u16,
    sessions: Arc<Mutex<HashMap<String, StreamingSession>>>,
}

impl TtsStreamingServer {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        
        // Find available port and bind once — no TOCTOU gap
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
        std_listener.set_nonblocking(true)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
        let port = std_listener.local_addr()
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?.port();
        
        let server = TtsStreamingServer {
            port,
            sessions: sessions.clone(),
        };
        
        // Start HTTP server. hyper 1.x dropped the `Server` builder, so the
        // accept loop is ours to run: take a connection, wrap it in the
        // hyper-util tokio adapter, and serve it on its own task.
        let sessions_clone = sessions.clone();
        let listener = tokio::net::TcpListener::from_std(std_listener)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;

        tokio::spawn(async move {
            loop {
                let stream = match listener.accept().await {
                    Ok((stream, _addr)) => stream,
                    Err(e) => {
                        eprintln!("TTS streaming server accept error: {}", e);
                        continue;
                    }
                };
                let sessions = sessions_clone.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |req| handle_request(req, sessions.clone()));
                    if let Err(e) = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await
                    {
                        eprintln!("TTS streaming server error: {}", e);
                    }
                });
            }
        });

        // Spawn idle cleanup task (every 60s remove sessions older than 60s that haven't started)
        let sessions_for_cleanup = sessions.clone();
        tokio::spawn(async move {
            let ttl = Duration::from_secs(60);
            let started_ttl = Duration::from_secs(300); // 5 min for active sessions
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let mut to_remove: Vec<String> = Vec::new();
                {
                    let guard = sessions_for_cleanup.lock().unwrap_or_else(|e| e.into_inner());
                    for (k, v) in guard.iter() {
                        let age = v.created_at.elapsed();
                        if age > ttl && !v.started.load(Ordering::SeqCst) {
                            to_remove.push(k.clone());
                        } else if age > started_ttl && v.started.load(Ordering::SeqCst) {
                            to_remove.push(k.clone());
                        }
                    }
                }
                if !to_remove.is_empty() {
                    let mut guard = sessions_for_cleanup.lock().unwrap_or_else(|e| e.into_inner());
                    for k in to_remove {
                        guard.remove(&k);
                    }
                }
            }
        });

        Ok(server)
    }
    
    
    pub fn create_session(&self, text: String, voice: String, model: String, format: String, api_key: String, instructions: Option<String>) -> String {
        let session_id = Uuid::new_v4().to_string();
        let session = StreamingSession {
            text,
            voice,
            model,
            format,
            api_key,
            instructions,
            cancel: Arc::new(AtomicBool::new(false)),
            created_at: Instant::now(),
            started: Arc::new(AtomicBool::new(false)),
        };
        
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.insert(session_id.clone(), session);
        session_id
    }
    
    pub fn stop_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sess) = sessions.get(session_id) {
            sess.cancel.store(true, Ordering::SeqCst);
        }
        sessions.remove(session_id).is_some()
    }
    
    pub fn get_stream_url(&self, session_id: &str) -> String {
        format!("http://127.0.0.1:{}/tts-stream/{}", self.port, session_id)
    }

    pub fn count_sessions(&self) -> usize {
        let guard = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        guard.len()
    }

    pub fn cleanup_idle(&self, ttl: Duration) -> usize {
        let mut removed = 0usize;
        let mut guard = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<String> = guard
            .iter()
            .filter_map(|(k, v)| {
                let age = v.created_at.elapsed();
                if age > ttl && !v.started.load(Ordering::SeqCst) {
                    Some(k.clone())
                } else { None }
            })
            .collect();
        for k in keys {
            guard.remove(&k);
            removed += 1;
        }
        removed
    }
}

async fn handle_request(
    req: Request<Incoming>,
    sessions: Arc<Mutex<HashMap<String, StreamingSession>>>,
) -> Result<Response<ResponseBody>, Infallible> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, path) if path.starts_with("/tts-stream/") => {
            let session_id = path.strip_prefix("/tts-stream/").unwrap_or("");
            handle_tts_stream(session_id, sessions).await
        }
        _ => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(fixed_body("Not Found"))
                .unwrap())
        }
    }
}

async fn handle_tts_stream(
    session_id: &str,
    sessions: Arc<Mutex<HashMap<String, StreamingSession>>>,
) -> Result<Response<ResponseBody>, Infallible> {
    // Get session details
    let (session_opt, cancel_flag, started_flag) = {
        let sessions_guard = sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = sessions_guard.get(session_id) {
            (Some(s.clone()), s.cancel.clone(), s.started.clone())
        } else { (None, Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false))) }
    };
    
    let session = match session_opt {
        Some(s) => s,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(fixed_body("Session not found"))
                .unwrap());
        }
    };
    
    // Mark started
    started_flag.store(true, Ordering::SeqCst);
    
    // Create OpenAI request
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| Client::new());
    // Build JSON body, omitting 'instructions' when not provided
    let mut body_obj = serde_json::Map::new();
    body_obj.insert("model".to_string(), serde_json::Value::String(session.model.clone()));
    body_obj.insert("input".to_string(), serde_json::Value::String(session.text.clone()));
    body_obj.insert("voice".to_string(), serde_json::Value::String(session.voice.clone()));
    body_obj.insert("response_format".to_string(), serde_json::Value::String(session.format.clone()));
    if let Some(instr) = &session.instructions {
        if !instr.trim().is_empty() {
            body_obj.insert("instructions".to_string(), serde_json::Value::String(instr.clone()));
        }
    }
    let body = serde_json::Value::Object(body_obj);
    
    let accept = match session.format.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "opus" => "audio/ogg",
        _ => "audio/mpeg",
    };

    let openai_response = match client
        .post("https://api.openai.com/v1/audio/speech")
        .bearer_auth(&session.api_key)
        .header("Accept", accept)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(fixed_body(format!("OpenAI request failed: {}", e)))
                .unwrap());
        }
    };
    
    if !openai_response.status().is_success() {
        let status = openai_response.status();
        let error_text = openai_response.text().await.unwrap_or_default();
        return Ok(Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(fixed_body(format!("OpenAI error {}: {}", status, error_text)))
            .unwrap());
    }
    
    // Determine content type based on format
    let content_type = match session.format.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "opus" => "audio/ogg",
        _ => "audio/mpeg", // default to mp3
    };
    
    // Stream the response with cancellation and cleanup on end
    let upstream = openai_response.bytes_stream();
    let sessions_for_body = sessions.clone();
    let session_id_string = session_id.to_string();
    let body_stream = futures_util::stream::unfold((upstream, cancel_flag, sessions_for_body, session_id_string, false), |(mut up, cancel, sessions_map, sid, cleaned)| async move {
        let cleaned_flag = cleaned;
        let maybe_cleanup = |sessions_map: &Arc<Mutex<HashMap<String, StreamingSession>>>, sid: &str, cleaned: &mut bool| {
            if !*cleaned {
                let mut guard = sessions_map.lock().unwrap_or_else(|e| e.into_inner());
                guard.remove(sid);
                *cleaned = true;
            }
        };
        if cancel.load(Ordering::SeqCst) {
            let mut c = cleaned_flag;
            maybe_cleanup(&sessions_map, &sid, &mut c);
            return None;
        }
        match up.next().await {
            Some(Ok(bytes)) => Some((Ok::<_, std::io::Error>(bytes), (up, cancel, sessions_map, sid, cleaned_flag))),
            Some(Err(e)) => {
                let mut c = cleaned_flag;
                maybe_cleanup(&sessions_map, &sid, &mut c);
                Some((Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())), (up, cancel, sessions_map, sid, c)))
            }
            None => {
                let mut c = cleaned_flag;
                maybe_cleanup(&sessions_map, &sid, &mut c);
                None
            }
        }
    });
    
    // Create response with streaming body. No explicit Transfer-Encoding
    // header: hyper 1.x picks the framing itself for a body of unknown length,
    // and setting it by hand here would fight that.
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header("Cache-Control", "no-cache")
        // Fully qualified: `StreamBody` is both a Body and a Stream here, and
        // both traits offer a `boxed`.
        .body(BodyExt::boxed(StreamBody::new(body_stream.map_ok(Frame::data))))
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the whole hyper wiring end to end - accept loop, service,
    /// routing and a fixed-size response body - without needing an API key or
    /// any network access. Both 404 branches go through the same body plumbing
    /// the streaming path uses.
    #[tokio::test]
    async fn unknown_session_returns_not_found() {
        let server = TtsStreamingServer::new().await.expect("server should start");
        let resp = reqwest::get(server.get_stream_url("no-such-session"))
            .await
            .expect("request should reach the local server");
        assert_eq!(resp.status().as_u16(), 404);
        assert_eq!(resp.text().await.expect("body"), "Session not found");
    }

    #[tokio::test]
    async fn unknown_path_returns_not_found() {
        let server = TtsStreamingServer::new().await.expect("server should start");
        let url = format!("http://127.0.0.1:{}/nope", server.port);
        let resp = reqwest::get(url).await.expect("request should reach the local server");
        assert_eq!(resp.status().as_u16(), 404);
        assert_eq!(resp.text().await.expect("body"), "Not Found");
    }

    #[test]
    fn create_and_stop_session_round_trip() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let server = TtsStreamingServer { port: 0, sessions };
        let id = server.create_session(
            "hello".into(),
            "verse".into(),
            "gpt-4o-mini-tts".into(),
            "mp3".into(),
            "sk-test".into(),
            None,
        );
        assert_eq!(server.count_sessions(), 1);
        assert!(server.stop_session(&id));
        assert_eq!(server.count_sessions(), 0);
        assert!(!server.stop_session(&id));
    }
}
