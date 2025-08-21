use tokio::{ net::TcpStream, io::{ AsyncWriteExt }, sync::RwLock };
use std::{ collections::HashMap, sync::Arc };

use serde::Serialize;

pub type Res = Arc<RwLock<Response>>;

pub trait ResponseExt {
    async fn with_write<F, Fut>(&self, f: F)
        where F: FnOnce(Res) -> Fut + Send, Fut: Future<Output = ()> + Send;
    async fn status(&self, code: u16);
    async fn send(&self, body: &str);
    async fn json<T: Serialize>(&self, data: &T);
}

impl ResponseExt for Res {
    async fn with_write<F, Fut>(&self, f: F)
        where F: FnOnce(Res) -> Fut + Send, Fut: Future<Output = ()> + Send
    {
        let res_clone = self.clone();

        f(res_clone.clone()).await;
    }

    async fn status(&self, code: u16) {
        let mut res = self.write().await;
        res.status(code).await;
    }

    async fn send(&self, body: &str) {
        let res = self.read().await;
        res.send(body).await;
    }

    async fn json<T: Serialize>(&self, data: &T) {
        let res = self.read().await;
        res.json(data).await;
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    stream: Arc<RwLock<TcpStream>>,
    status: u16,
    pub headers: Arc<RwLock<HashMap<String, String>>>,
    stopped: Arc<RwLock<bool>>,
}

impl Response {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: Arc::new(RwLock::new(stream)),
            status: 200,
            headers: Arc::new(RwLock::new(HashMap::new())),
            stopped: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn send_bytes(&self, bytes: &[u8], content_type: &str) {
        let headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
            self.status,
            get_status_text(self.status),
            content_type,
            bytes.len()
        );

        let mut stream = self.stream.write().await;

        let _ = stream.write_all(headers.as_bytes()).await;
        let _ = stream.write_all(bytes).await;

        self.stop().await;
    }

    pub async fn set_header(&self, key: &str, value: &str) {
        let mut headers = self.headers.write().await;
        headers.insert(key.to_string(), value.to_string());
    }

    pub async fn remove_header(&self, key: &str) {
        let mut headers = self.headers.write().await;
        headers.remove(key);
    }

    async fn stop(&self) {
        let mut s = self.stopped.write().await;
        *s = true;
    }

    pub async fn is_stopped(&self) -> bool {
        let stopped = *self.stopped.read().await;
        stopped.clone()
    }

    pub async fn status(&mut self, code: u16) {
        self.status = code;
    }

    pub async fn send(&self, body: &str) {
        let res = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n{}",
            self.status,
            get_status_text(self.status),
            body.len(),
            body
        );

        let mut stream = self.stream.write().await;
        let _ = stream.write_all(res.as_bytes()).await;
        // stream.flush().await;

        self.stop().await;
    }

    pub async fn json<T: Serialize>(&self, data: &T) {
        let body = serde_json::to_string(data).unwrap();

        let res = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\n\r\n{}",
            self.status,
            get_status_text(self.status),
            body.len(),
            body
        );

        let mut stream = self.stream.write().await;
        let _ = stream.write_all(res.as_bytes()).await;
        // stream.flush().await;

        self.stop().await;
    }
}

fn get_status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}
