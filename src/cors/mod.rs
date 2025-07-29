use std::sync::{ Arc, RwLock };

use crate::{ Next, Request, Response, RequestExt, ResponseExt };

pub trait CorsExt {
    fn run_middleware(&self, req: Arc<RwLock<Request>>, res: Arc<RwLock<Response>>, next: Next);
}

impl CorsExt for Arc<RwLock<Cors>> {
    fn run_middleware(&self, req: Arc<RwLock<Request>>, res: Arc<RwLock<Response>>, next: Next) {
        if let Ok(cors) = self.read() {
            cors.cors_middleware(req, res, next);
        } else {
            next();
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

    pub fn cors_middleware(
        &self,
        req: Arc<RwLock<Request>>,
        res: Arc<RwLock<Response>>,
        next: Next
    ) {
        let origin = req.with_read(|req| {
            req.headers.get("origin").cloned().unwrap_or_default()
        });

        let allow_all = self.allow_origins.contains(&"*".to_string());

        // Unlisted Origin
        if !allow_all && !self.allow_origins.contains(&origin) {
            res.with_write(|res| {
                res.status(401);
                res.set_header("Content-Type", "text/plain");
                res.send("Unauthorized origin");
            });
            return;
        }

        res.with_write(|res| {
            let allow_origin = if allow_all { "*" } else { &origin };
            res.set_header("Access-Control-Allow-Origin", allow_origin);
            res.set_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        });

        next();
    }
}
