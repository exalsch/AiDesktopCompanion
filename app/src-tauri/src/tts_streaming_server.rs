use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use hyper::{Body, Request, Response, Server, StatusCode, Method};
use hyper::service::{make_service_fn, service_fn};
use hyper::header::CONTENT_TYPE;
use reqwest::Client;
use serde_json;
use uuid::Uuid;
use futures_util::StreamExt;
use std::time::{Duration, Instant};

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
        
        // Find available port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        drop(listener);
        
        let server = TtsStreamingServer {
            port,
            sessions: sessions.clone(),
        };
        
        // Start HTTP server
        let sessions_clone = sessions.clone();
        let make_svc = make_service_fn(move |_conn| {
            let sessions = sessions_clone.clone();
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    handle_request(req, sessions.clone())
                }))
            }
        });
        
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let server_future = Server::bind(&addr).serve(make_svc);
        
        // Spawn server in background
        tokio::spawn(async move {
            if let Err(e) = server_future.await {
                eprintln!("TTS streaming server error: {}", e);
            }
        });

        // Spawn idle cleanup task (every 60s remove sessions older than 60s that haven't started)
        let sessions_for_cleanup = sessions.clone();
        tokio::spawn(async move {
            let ttl = Duration::from_secs(60);
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let mut to_remove: Vec<String> = Vec::new();
                {
                    let guard = sessions_for_cleanup.lock().unwrap();
                    for (k, v) in guard.iter() {
                        let age = v.created_at.elapsed();
                        if age > ttl && !v.started.load(Ordering::SeqCst) {
                            to_remove.push(k.clone());
                        }
                    }
                }
                if !to_remove.is_empty() {
                    let mut guard = sessions_for_cleanup.lock().unwrap();
                    for k in to_remove {
                        guard.remove(&k);
                    }
                }
            }
        });

        Ok(server)
    }
    
    pub fn get_port(&self) -> u16 {
        self.port
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
        
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session_id.clone(), session);
        session_id
    }
    
    pub fn stop_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(sess) = sessions.get(session_id) {
            sess.cancel.store(true, Ordering::SeqCst);
        }
        sessions.remove(session_id).is_some()
    }
    
    pub fn get_stream_url(&self, session_id: &str) -> String {
        format!("http://127.0.0.1:{}/tts-stream/{}", self.port, session_id)
    }

    pub fn count_sessions(&self) -> usize {
        let guard = self.sessions.lock().unwrap();
        guard.len()
    }

    pub fn cleanup_idle(&self, ttl: Duration) -> usize {
        let mut removed = 0usize;
        let mut guard = self.sessions.lock().unwrap();
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
    req: Request<Body>,
    sessions: Arc<Mutex<HashMap<String, StreamingSession>>>,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, path) if path.starts_with("/tts-stream/") => {
            let session_id = path.strip_prefix("/tts-stream/").unwrap_or("");
            handle_tts_stream(session_id, sessions).await
        }
        _ => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap())
        }
    }
}

async fn handle_tts_stream(
    session_id: &str,
    sessions: Arc<Mutex<HashMap<String, StreamingSession>>>,
) -> Result<Response<Body>, hyper::Error> {
    // Get session details
    let (session_opt, cancel_flag, started_flag) = {
        let sessions_guard = sessions.lock().unwrap();
        if let Some(s) = sessions_guard.get(session_id) {
            (Some(s.clone()), s.cancel.clone(), s.started.clone())
        } else { (None, Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false))) }
    };
    
    let session = match session_opt {
        Some(s) => s,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Session not found"))
                .unwrap());
        }
    };
    
    // Mark started
    started_flag.store(true, Ordering::SeqCst);
    
    // Create OpenAI request
    let client = Client::new();
    // Build JSON body, omitting 'instructions' when not provided
    let mut body_obj = serde_json::Map::new();
    body_obj.insert("model".to_string(), serde_json::Value::String(session.model.clone()));
    body_obj.insert("input".to_string(), serde_json::Value::String(session.text.clone()));
    body_obj.insert("voice".to_string(), serde_json::Value::String(session.voice.clone()));
    body_obj.insert("format".to_string(), serde_json::Value::String(session.format.clone()));
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
                .body(Body::from(format!("OpenAI request failed: {}", e)))
                .unwrap());
        }
    };
    
    if !openai_response.status().is_success() {
        let status = openai_response.status();
        let error_text = openai_response.text().await.unwrap_or_default();
        return Ok(Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Body::from(format!("OpenAI error {}: {}", status, error_text)))
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
    let body_stream = futures_util::stream::unfold((upstream, cancel_flag, sessions_for_body, session_id_string, false), |(mut up, cancel, sessions_map, sid, mut cleaned)| async move {
        if cancel.load(Ordering::SeqCst) {
            if !cleaned {
                let mut guard = sessions_map.lock().unwrap();
                guard.remove(&sid);
                cleaned = true;
            }
            return None;
        }
        match up.next().await {
            Some(Ok(bytes)) => Some((Ok::<_, std::io::Error>(bytes), (up, cancel, sessions_map, sid, cleaned))),
            Some(Err(e)) => {
                if !cleaned {
                    let mut guard = sessions_map.lock().unwrap();
                    guard.remove(&sid);
                    cleaned = true;
                }
                Some((Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())), (up, cancel, sessions_map, sid, cleaned)))
            }
            None => {
                if !cleaned {
                    let mut guard = sessions_map.lock().unwrap();
                    guard.remove(&sid);
                    cleaned = true;
                }
                None
            }
        }
    });
    
    // Create response with streaming body
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header("Cache-Control", "no-cache")
        .header("Transfer-Encoding", "chunked")
        .body(Body::wrap_stream(body_stream))
        .unwrap())
}
