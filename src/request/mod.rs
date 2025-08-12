use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::{ Arc };

pub type Req = Arc<RwLock<Request>>;

pub trait RequestExt {
    async fn with_write<F, Fut>(&self, f: F)
        where F: FnOnce(Req) -> Fut + Send, Fut: std::future::Future<Output = ()> + Send;

    async fn with_read<F, Fut, R>(&self, f: F) -> R
        where F: FnOnce(Req) -> Fut + Send, Fut: std::future::Future<Output = R> + Send, R: Send;

    async fn path(&self) -> Option<String>;
    async fn query(&self, key: &str) -> Option<String>;
    fn params(&self, key: &str) -> impl std::future::Future<Output = Option<String>> + Send;
    async fn body(&self) -> Option<String>;
}

impl RequestExt for Req {
    async fn with_write<F, Fut>(&self, f: F)
        where F: FnOnce(Req) -> Fut + Send, Fut: Future<Output = ()> + Send
    {
        let req_clone = self.clone();
        f(req_clone.clone()).await;
    }

    async fn with_read<F, Fut, R>(&self, f: F) -> R
        where F: FnOnce(Req) -> Fut + Send, Fut: Future<Output = R> + Send, R: Send
    {
        let req_clone = self.clone();

        f(req_clone.clone()).await
    }
    async fn path(&self) -> Option<String> {
        let req = self.read().await;
        Some(req.path.clone())
    }
    async fn query(&self, key: &str) -> Option<String> {
        self.read().await.query(key).cloned()
    }

    async fn params(&self, key: &str) -> Option<String> {
        self.read().await.params(key).cloned()
    }

    async fn body(&self) -> Option<String> {
        self.read().await.body.clone()
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub path_params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: Option<String>,
    pub headers: HashMap<String, String>,
}

impl Request {
    pub fn new(req: &[String]) -> Self {
        let (method, full_path) = {
            let parts: Vec<&str> = req[0].split_whitespace().collect();
            (parts[0].to_string(), parts[1])
        };

        let (path, query) = if let Some(pos) = full_path.find('?') {
            (full_path[..pos].to_string(), parse_query(&full_path[pos + 1..]))
        } else {
            (full_path.to_string(), HashMap::new())
        };

        let mut headers = HashMap::<String, String>::new();
        let mut body_lines = Vec::new();
        let mut is_body = false;

        for line in req[1..].iter() {
            if is_body {
                body_lines.push(line.clone());
                continue;
            }

            if line.is_empty() {
                is_body = true;
                continue;
            }

            if let Some((k, v)) = line.split_once(": ") {
                headers.insert(k.to_string().to_lowercase(), v.to_string());
            }
        }

        let body = if body_lines.is_empty() { None } else { Some(body_lines.join("\n")) };

        Self {
            method,
            path,
            path_params: HashMap::new(),
            query,
            body,
            headers,
        }
    }

    pub fn query(&self, key: &str) -> Option<&String> {
        self.query.get(key)
    }

    pub fn params(&self, key: &str) -> Option<&String> {
        self.path_params.get(key)
    }
}

fn parse_query(query_line: &str) -> HashMap<String, String> {
    let mut querys = HashMap::<String, String>::new();

    for query in query_line.split('&') {
        let mut parts = query.splitn(2, '=');
        if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
            querys.insert(key.to_string(), val.to_string());
        }
    }

    querys
}

pub fn parse_path_params(
    route_pattern: &str,
    actual_path: &str
) -> Option<HashMap<String, String>> {
    let mut params = HashMap::new();

    let pattern_parts = route_pattern.trim_matches('/').split('/');
    let path_parts = actual_path.trim_matches('/').split('/');

    let mut pattern_iter = pattern_parts.peekable();
    let mut path_iter = path_parts.peekable();

    while let (Some(pattern), Some(actual)) = (pattern_iter.next(), path_iter.next()) {
        if pattern.starts_with(':') {
            params.insert(pattern[1..].to_string(), actual.to_string());
        } else if pattern != actual {
            return None;
        }
    }

    if pattern_iter.next().is_some() || path_iter.next().is_some() {
        return None;
    }

    Some(params)
}
