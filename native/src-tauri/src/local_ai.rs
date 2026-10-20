//! Ollama adapter: literal loopback, no proxy/redirects, verified local weights,
//! bounded streaming and cancellation while connecting, evaluating and generating.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
const MAX_BODY: usize = 4_000_000;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub name: String,
    pub size: u64,
    pub digest: String,
    pub context: u32,
    #[serde(skip)]
    pub template_bytes: usize,
}
pub struct Prompt<'a> {
    pub model: &'a Model,
    pub context: u32,
    pub system: &'a str,
    pub input: &'a str,
    pub structured: bool,
}
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    port: u16,
}
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("Local AI cancelled. Saved transcripts and earlier results were preserved.".into())
    } else {
        Ok(())
    }
}
async fn wait<T>(
    future: impl Future<Output = Result<T, reqwest::Error>>,
    cancel: &AtomicBool,
) -> Result<T, String> {
    tokio::pin!(future);
    loop {
        tokio::select! {result=&mut future=>return result.map_err(|e|format!("Local Ollama request failed. Start Ollama, check the port and installed model, then retry: {e}")),_=tokio::time::sleep(Duration::from_millis(100))=>cancelled(cancel)?}
    }
}
impl Client {
    pub fn new(port: u16) -> Result<Self, String> {
        if port < 1024 {
            return Err("Choose a local Ollama port between 1024 and 65535.".into());
        }
        Ok(Self {
            port,
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .http1_only()
                .pool_max_idle_per_host(0)
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(600))
                .build()
                .map_err(|e| e.to_string())?,
        })
    }
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
    async fn response(
        &self,
        path: &str,
        body: Option<&Value>,
        cancel: &AtomicBool,
    ) -> Result<reqwest::Response, String> {
        cancelled(cancel)?;
        let request = if let Some(body) = body {
            self.http.post(self.url(path)).json(body)
        } else {
            self.http.get(self.url(path))
        };
        let response = wait(request.send(), cancel).await?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(format!("Local Ollama returned HTTP {}. Check model availability and the local service; redirects and remote providers are not supported.",response.status().as_u16()));
        }
        Ok(response)
    }
    async fn json(
        &self,
        path: &str,
        body: Option<&Value>,
        cancel: &AtomicBool,
    ) -> Result<Value, String> {
        let mut response = self.response(path, body, cancel).await?;
        let mut data = vec![];
        while let Some(bytes) = wait(response.chunk(), cancel).await? {
            if data.len() + bytes.len() > MAX_BODY {
                return Err("Local model metadata exceeded the supported size.".into());
            }
            data.extend_from_slice(&bytes);
        }
        serde_json::from_slice(&data).map_err(|e| format!("Ollama returned invalid JSON: {e}"))
    }
    pub async fn models(&self, cancel: &AtomicBool) -> Result<Vec<Model>, String> {
        let response = self.json("/api/tags", None, cancel).await?;
        let tags = response
            .get("models")
            .and_then(Value::as_array)
            .ok_or("Ollama did not return its installed model list.")?;
        if tags.len() > 500 {
            return Err("Ollama returned too many installed models.".into());
        }
        let mut output = vec![];
        for tag in tags {
            let Some(name) = tag.get("name").and_then(Value::as_str) else {
                continue;
            };
            if local_tag(name).is_err()
                || tag.get("size").and_then(Value::as_u64).unwrap_or(0) < 1_000_000
                || tag.pointer("/details/format").and_then(Value::as_str) != Some("gguf")
                || remote(tag)
            {
                continue;
            }
            let Some(digest) = tag.get("digest").and_then(Value::as_str) else {
                continue;
            };
            if !digest
                .strip_prefix("sha256:")
                .unwrap_or(digest)
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
                || digest.strip_prefix("sha256:").unwrap_or(digest).len() != 64
            {
                continue;
            }
            output.push(Model {
                name: name.into(),
                size: tag["size"].as_u64().unwrap_or(0),
                digest: digest.into(),
                context: 0,
                template_bytes: 0,
            });
        }
        Ok(output)
    }
    pub async fn inspect(&self, name: &str, cancel: &AtomicBool) -> Result<Model, String> {
        local_tag(name)?;
        let mut model=self.models(cancel).await?.into_iter().find(|model|model.name==name).ok_or("Choose an installed local GGUF model. Cloud-backed and missing models are not accepted.")?;
        let show = self
            .json("/api/show", Some(&json!({"model":name})), cancel)
            .await?;
        let (context, template_bytes) = validate_show(&show)?;
        model.context = context;
        model.template_bytes = template_bytes;
        Ok(model)
    }
    pub async fn generate(
        &self,
        request: Prompt<'_>,
        cancel: &AtomicBool,
        mut progress: impl FnMut(&str),
    ) -> Result<String, String> {
        let Prompt {
            model,
            context,
            system,
            input,
            structured,
        } = request;
        let current = self.inspect(&model.name, cancel).await?;
        if current.digest != model.digest {
            return Err(
                "The installed model changed during generation. Refresh models and start again."
                    .into(),
            );
        }
        let output = 2048_u32;
        let budget = budget(&current, context, output)?;
        if system.len() + input.len() > budget {
            return Err("This request exceeds the conservative local-model context budget. Choose a larger context or shorten the prompt; no transcript content was silently dropped.".into());
        }
        let mut body = json!({"model":current.name,"messages":[{"role":"system","content":system},{"role":"user","content":input}],"stream":true,"think":false,"keep_alive":0,"options":{"num_ctx":context.min(current.context),"num_predict":output,"temperature":0}});
        if structured {
            body["format"] = json!({"type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"id":{"type":"integer"},"text":{"type":"string"}},"required":["id","text"],"additionalProperties":false}}},"required":["items"],"additionalProperties":false});
        }
        let mut response = self.response("/api/chat", Some(&body), cancel).await?;
        let mut pending = vec![];
        let mut text = String::new();
        let mut total = 0;
        let mut done = false;
        while let Some(bytes) = wait(response.chunk(), cancel).await? {
            total += bytes.len();
            if total > MAX_BODY {
                return Err("Local AI response exceeded its size limit.".into());
            }
            pending.extend_from_slice(&bytes);
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=index).collect();
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let value: Value = serde_json::from_slice(&line)
                    .map_err(|e| format!("Invalid streamed response: {e}"))?;
                done |= piece(&value, &mut text)?;
                progress(&text);
            }
            if pending.len() > 500_000 {
                return Err("Local AI returned an oversized stream record.".into());
            }
        }
        if !pending.is_empty() {
            let value: Value = serde_json::from_slice(&pending).map_err(|e| e.to_string())?;
            done |= piece(&value, &mut text)?;
            progress(&text);
        }
        cancelled(cancel)?;
        if !done || text.trim().is_empty() {
            return Err("Local AI response ended before a complete answer. Retry; your source was preserved.".into());
        }
        Ok(text.trim().into())
    }
}
fn remote(value: &Value) -> bool {
    ["remote_host", "remote_model"].iter().any(|field| {
        value
            .get(field)
            .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    })
}
pub fn local_tag(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 200
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:./".contains(&byte))
        || (name.to_ascii_lowercase().ends_with(":cloud")
            || name.to_ascii_lowercase().ends_with("-cloud"))
        || name.contains("://")
    {
        Err("Choose an installed local model; cloud model names and URLs are not accepted.".into())
    } else {
        Ok(())
    }
}
fn validate_show(value: &Value) -> Result<(u32, usize), String> {
    if remote(value)
        || value.pointer("/details/format").and_then(Value::as_str) != Some("gguf")
        || !value
            .get("capabilities")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("completion")))
    {
        return Err("This model is not verified as a local text-generation model. Cloud, embedding-only and unsupported models are refused.".into());
    }
    let info = value
        .get("model_info")
        .and_then(Value::as_object)
        .ok_or("Ollama did not return local weight metadata. Update Ollama and retry.")?;
    if info
        .get("general.parameter_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        == 0
        || info
            .get("general.architecture")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
    {
        return Err("No local model weights were reported. Cloud routing is not supported.".into());
    }
    let context = info
        .iter()
        .filter(|(key, _)| key.ends_with(".context_length"))
        .filter_map(|(_, value)| value.as_u64())
        .min()
        .ok_or("This local model did not declare a context length.")?
        .min(131072) as u32;
    let template = value
        .get("template")
        .and_then(Value::as_str)
        .unwrap_or("")
        .len()
        + value
            .get("system")
            .and_then(Value::as_str)
            .unwrap_or("")
            .len();
    if template > 64000
        || value
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| !messages.is_empty())
    {
        return Err("Models with large templates or built-in conversation history are not supported. Choose a standard local instruct model.".into());
    }
    Ok((context, template))
}
pub fn budget(model: &Model, context: u32, output: u32) -> Result<usize, String> {
    if !(4096..=32768).contains(&context) {
        return Err("Choose a local context window between 4096 and 32768 tokens.".into());
    }
    context.min(model.context).checked_sub(output+model.template_bytes as u32+512).filter(|value|*value>=1024).map(|value|value as usize).ok_or("This model/context leaves too little room for transcript content. Choose a larger context or another model.".into())
}
fn piece(value: &Value, text: &mut String) -> Result<bool, String> {
    if remote(value) {
        return Err(
            "Ollama reported a remote model. Generation stopped; only local weights are supported."
                .into(),
        );
    }
    if value.get("error").is_some() {
        return Err("The local model reported a generation error. Check Ollama and retry.".into());
    }
    if value.get("done_reason").and_then(Value::as_str) == Some("length") {
        return Err("Local generation reached its output limit. Retry with a shorter prompt or another model; incomplete output was not marked complete.".into());
    }
    if let Some(part) = value.pointer("/message/content").and_then(Value::as_str) {
        if text.len() + part.len() > 1_000_000 {
            return Err("Generated text is too large.".into());
        }
        text.push_str(part);
    }
    Ok(value.get("done").and_then(Value::as_bool) == Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };
    fn show() -> Value {
        json!({"details":{"format":"gguf"},"capabilities":["completion"],"model_info":{"general.architecture":"test","general.parameter_count":1500000,"test.context_length":32768},"template":"{{.Prompt}}"})
    }
    fn tags() -> Value {
        json!({"models":[{"name":"test:latest","size":2000000,"digest":"a".repeat(64),"details":{"format":"gguf"}}]})
    }
    fn server(
        replies: Vec<(u16, Vec<u8>)>,
    ) -> (Client, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = Client::new(listener.local_addr().unwrap().port()).unwrap();
        let requests = Arc::new(Mutex::new(vec![]));
        let saved = requests.clone();
        let handle = thread::spawn(move || {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut data = vec![];
                let mut byte = [0u8; 1];
                while !data.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    data.push(byte[0]);
                }
                let header = String::from_utf8(data.clone()).unwrap();
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut payload = vec![0; length];
                stream.read_exact(&mut payload).unwrap();
                data.extend(payload);
                saved.lock().unwrap().push(String::from_utf8(data).unwrap());
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:1/forbidden\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                for chunk in body.chunks(2) {
                    if stream.write_all(chunk).is_err() {
                        break;
                    }
                }
            }
        });
        (client, requests, handle)
    }
    fn json_response(value: Value) -> (u16, Vec<u8>) {
        (200, value.to_string().into_bytes())
    }
    #[test]
    fn refuses_cloud_weights_urls_and_insufficient_context() {
        for tag in ["https://api.example/model", "gpt:cloud", "thing-cloud", ""] {
            assert!(local_tag(tag).is_err());
        }
        assert!(local_tag("qwen2.5:1.5b").is_ok());
        let mut metadata = show();
        assert!(validate_show(&metadata).is_ok());
        metadata["remote_host"] = json!("https://example.com");
        assert!(validate_show(&metadata).is_err());
        metadata = show();
        metadata["model_info"] = json!({});
        assert!(validate_show(&metadata).is_err());
        let model = Model {
            name: "test:latest".into(),
            size: 2000000,
            digest: "a".repeat(64),
            context: 2048,
            template_bytes: 0,
        };
        assert!(budget(&model, 8192, 2048).is_err());
        let mut text = String::new();
        assert!(piece(&json!({"done":true,"done_reason":"length"}), &mut text).is_err());
        assert!(piece(&json!({"remote_model":"cloud"}), &mut text).is_err());
    }
    #[tokio::test]
    async fn fragmented_unicode_stream_is_complete_and_request_is_local() {
        let body = format!(
            "{}\n{}\n",
            json!({"message":{"content":"Привіт 🌍"},"done":false}),
            json!({"done":true})
        )
        .into_bytes();
        let (client, requests, handle) = server(vec![
            json_response(tags()),
            json_response(show()),
            json_response(tags()),
            json_response(show()),
            (200, body),
        ]);
        let cancel = AtomicBool::new(false);
        let model = client.inspect("test:latest", &cancel).await.unwrap();
        let output = client
            .generate(
                Prompt {
                    model: &model,
                    context: 8192,
                    system: "Summarize",
                    input: "Private text",
                    structured: false,
                },
                &cancel,
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(output, "Привіт 🌍");
        handle.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[4].starts_with("POST /api/chat "));
        let payload: Value =
            serde_json::from_str(requests[4].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(payload["keep_alive"], 0);
        assert_eq!(payload["think"], false);
        assert_eq!(payload["messages"][1]["content"], "Private text");
    }
    #[tokio::test]
    async fn rejects_remote_before_sending_source_redirect_and_truncated_generation() {
        let mut remote = show();
        remote["remote_model"] = json!("remote");
        let (client, requests, handle) = server(vec![json_response(tags()), json_response(remote)]);
        assert!(client
            .inspect("test:latest", &AtomicBool::new(false))
            .await
            .is_err());
        handle.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 2);
        let (client, requests, handle) = server(vec![(302, vec![])]);
        assert!(client.models(&AtomicBool::new(false)).await.is_err());
        handle.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
        let (client, _, handle) = server(vec![
            json_response(tags()),
            json_response(show()),
            (200, b"{\"message\":{\"content\":\"partial\"}}\n".to_vec()),
        ]);
        let model = Model {
            name: "test:latest".into(),
            size: 2000000,
            digest: "a".repeat(64),
            context: 32768,
            template_bytes: 0,
        };
        assert!(client
            .generate(
                Prompt {
                    model: &model,
                    context: 8192,
                    system: "Task",
                    input: "Source",
                    structured: false
                },
                &AtomicBool::new(false),
                |_| {}
            )
            .await
            .unwrap_err()
            .contains("complete answer"));
        handle.join().unwrap();
    }
    #[tokio::test]
    async fn cancellation_interrupts_waiting_for_headers_and_body() {
        for body in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let client = Client::new(listener.local_addr().unwrap().port()).unwrap();
            let handle = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut data = [0; 4096];
                let _ = stream.read(&mut data);
                if body {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\n\r\n{")
                        .unwrap();
                }
                thread::sleep(Duration::from_millis(500));
            });
            let cancel = AtomicBool::new(false);
            let (result, ()) = tokio::join!(client.models(&cancel), async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                cancel.store(true, Ordering::SeqCst);
            });
            assert!(result.unwrap_err().contains("cancelled"));
            handle.join().unwrap();
        }
    }
    #[tokio::test]
    #[ignore = "Requires an explicitly configured, cloud-disabled local Ollama and installed model"]
    async fn real_local_ai_pipeline() {
        let port = std::env::var("GOLOS_TEST_OLLAMA_PORT")
            .unwrap()
            .parse()
            .unwrap();
        let name = std::env::var("GOLOS_TEST_OLLAMA_MODEL").unwrap();
        let (db, mut request) = crate::ai::tests::fixture();
        let client = Client::new(port).unwrap();
        let cancel = AtomicBool::new(false);
        let model = client.inspect(&name, &cancel).await.unwrap();
        for kind in ["summary", "actions", "chat", "cleanup", "translate"] {
            request.kind = kind.into();
            request.target = if kind == "translate" {
                "Ukrainian".into()
            } else {
                String::new()
            };
            request.prompt = if kind == "chat" {
                "When will we ship?".into()
            } else {
                String::new()
            };
            let mut work = crate::ai::create(&db, &request, &name, 16384).unwrap();
            crate::ai_generation::generate(&mut work, &client, &model, "", &cancel, |work| {
                crate::ai::checkpoint(&db, work)
            })
            .await
            .unwrap();
            assert!(!work.content.trim().is_empty());
            if ["cleanup", "translate"].contains(&kind) {
                assert_eq!(work.segments.len(), 1);
                assert_eq!(work.segments[0].start_ms, 0);
                assert_eq!(work.segments[0].speaker.as_deref(), Some("Alice"));
            }
            println!("{kind}: {}", work.content);
        }
        assert_eq!(
            crate::library::get(&db, "source").unwrap().segments[0].text,
            "We agreed to ship on Friday."
        );
    }
}
