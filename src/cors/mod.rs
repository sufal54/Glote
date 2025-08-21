use std::sync::{ Arc };
use tokio::sync::RwLock;

use crate::{ Next, Req, Res, ResponseExt };

pub trait CorsExt {
    async fn run_middleware(&self, req: Req, res: Res, next: Next);
}

impl CorsExt for Arc<RwLock<Cors>> {
    async fn run_middleware(&self, req: Req, res: Res, next: Next) {
        match self.try_read() {
            Ok(cors) => {
                cors.cors_middleware(req, res, next).await;
            }
            Err(_) => {
                next().await;
            }
        }
    }
}

pub struct Cors {
    allow_origins: Vec<String>,
}

impl Cors {
    pub fn new(allow_origins: &[&str]) -> Arc<RwLock<Self>> {
        Arc::new(
            RwLock::new(Self {
                allow_origins: allow_origins
                    .iter()
                    .map(|origins| origins.to_string())
                    .collect(),
            })
        )
    }

    pub async fn cors_middleware(&self, req: Req, res: Res, next: Next) {
        let origin = {
            let req_read = req.read().await;
            req_read.headers.get("origin").cloned().unwrap_or_default()
        };

        let allow_all = self.allow_origins.contains(&"*".to_string());

        // Case Unlisted Origin
        if !allow_all && !self.allow_origins.contains(&origin) {
            res.with_write(|res| async move {
                let mut res = res.write().await;
                res.status(401).await;
                res.set_header("Content-Type", "text/plain").await;
                res.send("Unauthorized origin").await;
            }).await;
            return;
        }

        res.with_write(|res| async move {
            let res = res.write().await;
            let allow_origin = if allow_all { "*" } else { &origin };
            res.set_header("Access-Control-Allow-Origin", allow_origin).await;
            res.set_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS").await;
        }).await;

        next().await;
    }
}
