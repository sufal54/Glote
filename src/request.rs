use std::collections::HashMap;

use std::sync::{ Arc, RwLock };

pub trait RequestExt {
    fn with_write<F>(&self, f: F) where F: FnOnce(&mut Request);
    fn with_read<F, R>(&self, f: F) -> R where F: FnOnce(&Request) -> R;

    fn path(&self) -> Option<String>;
    fn query(&self, key: &str) -> Option<String>;
    fn params(&self, key: &str) -> Option<String>;
    fn body(&self) -> Option<String>;
}

impl RequestExt for Arc<RwLock<Request>> {
    fn with_write<F>(&self, f: F) where F: FnOnce(&mut Request) {
        if let Ok(mut req) = self.write() {
            f(&mut req);
        }
    }
    fn with_read<F, R>(&self, f: F) -> R where F: FnOnce(&Request) -> R {
        let req = self.read().unwrap();
        f(&req)
    }
    fn path(&self) -> Option<String> {
        if let Ok(req) = self.read() { Some(req.path.clone()) } else { None }
    }
    fn query(&self, key: &str) -> Option<String> {
        self.read().ok()?.query(key).cloned()
    }

    fn params(&self, key: &str) -> Option<String> {
        self.read().ok()?.params(key).cloned()
    }

    fn body(&self) -> Option<String> {
        self.read().ok()?.body.clone()
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
