use std::{ collections::HashMap, io::Write, net::TcpStream, sync::{ Arc, RwLock } };

use serde::Serialize;

pub trait ResponseExt {
    fn with_write<F>(&self, f: F) where F: FnOnce(&mut Response);
    fn status(&self, code: u16);
    fn send(&self, body: &str);
    fn json<T: Serialize>(&self, data: &T);
}

impl ResponseExt for Arc<RwLock<Response>> {
    fn with_write<F>(&self, f: F) where F: FnOnce(&mut Response) {
        if let Ok(mut res) = self.write() {
            f(&mut res);
        }
    }

    fn status(&self, code: u16) {
        if let Ok(mut res) = self.write() {
            res.status(code);
        }
    }

    fn send(&self, body: &str) {
        if let Ok(res) = self.read() {
            res.send(body);
        }
    }

    fn json<T: Serialize>(&self, data: &T) {
        if let Ok(res) = self.read() {
            res.json(data);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    stream: Arc<RwLock<TcpStream>>,
    status: u16,
    headers: Arc<RwLock<HashMap<String, String>>>,
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

    pub fn set_header(&self, key: &str, value: &str) {
        if let Ok(mut headers) = self.headers.write() {
            headers.insert(key.to_string(), value.to_string());
        }
    }

    pub fn remove_header(&self, key: &str) {
        if let Ok(mut headers) = self.headers.write() {
            headers.remove(key);
        }
    }

    fn stop(&self) {
        let _ = self.stopped.write().map(|mut s| {
            *s = true;
        });
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
            .read()
            .map(|s| *s)
            .unwrap_or(false)
    }

    pub fn status(&mut self, code: u16) {
        self.status = code;
    }

    pub fn send(&self, body: &str) {
        let res = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n{}",
            self.status,
            get_status_text(self.status),
            body.len(),
            body
        );

        if let Ok(mut stream) = self.stream.write() {
            let _ = stream.write_all(res.as_bytes());
            stream.flush().unwrap();
        }
        self.stop();
    }

    pub fn json<T: Serialize>(&self, data: &T) {
        let body = serde_json::to_string(data).unwrap();

        let res = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\n\r\n{}",
            self.status,
            get_status_text(self.status),
            body.len(),
            body
        );

        if let Ok(mut stream) = self.stream.write() {
            let _ = stream.write_all(res.as_bytes());
            stream.flush().unwrap();
        }
        self.stop();
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
