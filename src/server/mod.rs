use tokio::{
    fs::File,
    io::{ AsyncBufReadExt, AsyncReadExt, BufReader, ErrorKind },
    net::TcpListener,
    runtime::Runtime,
    sync::RwLock,
};
use std::{ future::Future, path::PathBuf, pin::Pin };
use std::sync::{ Arc };
use std::time::Instant;
use mime_guess;

pub mod macros;

use crate::request::{ parse_path_params, Request };
use crate::response::Response;
// use crate::workerpool::WorkerPool;

pub type Next = Box<dyn (FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>>) + Send + Sync>;

pub type Middleware = dyn (Fn(
    Arc<RwLock<Request>>,
    Arc<RwLock<Response>>,
    Next
) -> Pin<Box<dyn Future<Output = ()> + Send>>) +
    Send +
    Sync;

pub type Handler = dyn (Fn(
    Arc<RwLock<Request>>,
    Arc<RwLock<Response>>
) -> Pin<Box<dyn Future<Output = ()> + Send>>) +
    Send +
    Sync;

// Metadata of routes
#[derive(Clone)]
struct Route {
    method: String,
    path: String,
    middleware: Vec<Arc<Middleware>>,
    handler: Arc<Handler>,
}

pub struct Glote {
    routes: Arc<RwLock<Vec<Route>>>,
    middleware: Arc<RwLock<Vec<Arc<Middleware>>>>,
    // pool: WorkerPool,
    static_path: Arc<RwLock<Option<String>>>,
    runtime: Runtime,
}

impl Glote {
    // Returns Arc self
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            routes: Arc::new(RwLock::new(Vec::new())),
            middleware: Arc::new(RwLock::new(Vec::new())),
            static_path: Arc::new(RwLock::new(None)),
            runtime: tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime"),
        })
    }

    pub fn block_on<F: Future>(&self, fut: F) -> F::Output {
        self.runtime.block_on(fut)
    }

    pub async fn static_path(&self, path: &str) {
        let static_path = Arc::clone(&self.static_path);
        *static_path.write().await = Some(path.into());
    }

    // Runs Global+route middleware and final handler
    async fn run_handlers(
        &self,
        req: Arc<RwLock<Request>>,
        res: Arc<RwLock<Response>>,
        middlewares: &[Arc<Middleware>],
        final_handler: Arc<Handler>
    ) {
        fn call_middleware(
            req: Arc<RwLock<Request>>,
            res: Arc<RwLock<Response>>,
            middlewares: &[Arc<Middleware>],
            idx: usize,
            final_handler: Arc<Handler>
        ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            if idx == middlewares.len() {
                Box::pin(final_handler(req, res))
            } else {
                let mw = middlewares[idx].clone();
                let new_req = req.clone();
                let new_res = res.clone();
                let new_middleware = middlewares.to_vec();
                let new_final_handler = final_handler.clone();

                let next: Next = Box::new(move || {
                    Box::pin(
                        call_middleware(
                            new_req.clone(),
                            new_res.clone(),
                            &new_middleware,
                            idx + 1,
                            new_final_handler.clone()
                        )
                    )
                });

                Box::pin(async move {
                    mw(req, res, next).await;
                })
            }
        }

        call_middleware(req, res, middlewares, 0, final_handler).await;
    }

    // Set Global Middleware
    pub async fn use_middleware<F, Fut>(&self, middleware: F)
        where
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = ()> + Send + 'static
    {
        let wrapped = move |req, res, next| {
            Box::pin(middleware(req, res, next)) as Pin<Box<dyn Future<Output = ()> + Send>>
        };

        let mut middlewares = self.middleware.write().await;
        middlewares.push(Arc::new(wrapped));
    }

    /**
     * Start our server at specific port
     */
    pub async fn listen(self: Arc<Self>, port: u16) -> tokio::io::Result<()> {
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;

        println!("\n---------------------\nServer running on port {}", port);

        let global_middleware = self.middleware.read().await.clone();

        for route in self.routes.write().await.iter_mut() {
            let mut new_middleware = global_middleware.clone();

            let route_specific = std::mem::take(&mut route.middleware);

            new_middleware.extend(route_specific);
            route.middleware = new_middleware;
        }

        drop(global_middleware);

        // Listening incoming request
        loop {
            match listener.accept().await {
                Ok((s, _add)) => {
                    // Filter out raw stream from inconging request
                    let stream = s;
                    // Clone of our Routes
                    let routers_clone = {
                        let guard = self.routes.read().await;
                        guard.clone()
                    };
                    // static file not used
                    let static_file = self.static_path.clone();

                    let this = self.clone();
                    // Assign a Worker though warkerpool
                    tokio::spawn(async move {
                        // Current time for time takes to fullfill the request
                        let now = Instant::now();
                        // Shadowing make mutable
                        let mut stream = stream;
                        // TcpStream to buffer stream
                        let mut reader = BufReader::new(&mut stream);
                        // Request data Header and Body
                        let mut lines = Vec::new();
                        // Buffer stream store as Chunk of string
                        let mut buffer = String::new();

                        loop {
                            buffer.clear();
                            match reader.read_line(&mut buffer).await {
                                Ok(0) => {
                                    break;
                                }
                                Ok(_) => {
                                    let line = buffer.trim_end().to_string();
                                    if line.is_empty() {
                                        break;
                                    }
                                    lines.push(line);
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                                    continue;
                                }
                                Err(ref e) if e.kind() == ErrorKind::Interrupted => {
                                    continue;
                                }
                                Err(e) => {
                                    eprintln!("Failed to read line: {e}");
                                    return;
                                }
                            }
                        }
                        // Length of request content
                        let content_length = lines
                            .iter()
                            .find(|line| line.to_ascii_lowercase().starts_with("content-length:"))
                            .and_then(|line| line.split(": ").nth(1))
                            .and_then(|len| len.parse::<usize>().ok());
                        // Store body as Vec line
                        let mut body_lines = Vec::new();
                        // Case have length
                        if let Some(len) = content_length {
                            // Make buffer to store full content
                            let mut buf = vec![0u8; len];
                            // Store data into buf
                            match reader.read_exact(&mut buf).await {
                                Ok(_) => {
                                    let body = String::from_utf8_lossy(&buf).to_string();
                                    body_lines.extend(body.lines().map(|s| s.to_string()));
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                                    return;
                                }
                                Err(e) => {
                                    eprintln!("Failed to read body: {e}");
                                    return;
                                }
                            }

                            // Parse into UTF_8
                            let body = String::from_utf8_lossy(&buf).to_string();
                            // Concat it in body_lines
                            body_lines.extend(body.lines().map(|s| s.to_string()));
                        }

                        lines.push(String::new()); // Empty string before body
                        lines.extend(body_lines);

                        // Parse metadata into Request struct
                        let req = Request::new(&lines);
                        // Parse stream into Response struct
                        let mut res_opt = Some(Arc::new(RwLock::new(Response::new(stream))));
                        // Check is Route have or not
                        let mut matched = false;
                        // Iterate in Routes
                        for route in routers_clone.into_iter() {
                            // Case method same
                            if route.method == req.method {
                                // Parse params
                                if let Some(params) = parse_path_params(&route.path, &req.path) {
                                    // CLone req inside have params
                                    let mut req_with_params = req.clone();
                                    req_with_params.path_params = params;
                                    let req_with_params = Arc::new(RwLock::new(req_with_params));

                                    // Combined Global Middleware and Routes Middleware
                                    let combined_middleware: Vec<_> = route.middleware.clone();

                                    if let Some(res_actual) = res_opt.take() {
                                        // Move ownership
                                        let req_for_handler = Arc::clone(&req_with_params);
                                        let res_for_handler = Arc::clone(&res_actual);
                                        // Call run_handler
                                        this.run_handlers(
                                            Arc::clone(&req_for_handler),
                                            Arc::clone(&res_for_handler),
                                            &combined_middleware,
                                            route.handler.clone()
                                        ).await;

                                        matched = true;
                                        break;
                                    }
                                }
                            }
                        }
                        // Duration to fullfill the request
                        let duration = now.elapsed();

                        // Case route not matched
                        if !matched {
                            if let Some(res) = res_opt {
                                if let Some(static_dir) = &static_file.read().await.as_ref() {
                                    let mut file_path = PathBuf::from(static_dir);
                                    let mut req_path = req.path.trim_start_matches('/').to_string();

                                    if req_path.is_empty() {
                                        req_path = "index.html".into();
                                    }

                                    file_path.push(req_path);

                                    if let Ok(mut file) = File::open(&file_path).await {
                                        let mut contents = Vec::new();
                                        if file.read_to_end(&mut contents).await.is_ok() {
                                            let mut res = res.write().await;
                                            res.status(200).await;
                                            res.send_bytes(
                                                &contents,
                                                mime_guess
                                                    ::from_path(&file_path)
                                                    .first_or_text_plain()
                                                    .as_ref()
                                            ).await;
                                            println!(
                                                "\x1b[34mSTATIC {}: {:?}\x1b[0m",
                                                file_path.display(),
                                                duration
                                            );
                                            return;
                                        }
                                    }
                                }

                                let mut res = res.write().await;
                                res.status(404).await;
                                res.send("404 Not Found").await;
                            }
                            println!("\x1b[31m{} {}: {:?}\x1b[0m ", req.method, req.path, duration);
                        } else {
                            println!("\x1b[32m{} {}: {:?}\x1b[0m ", req.method, req.path, duration);
                        }
                    });
                }
                Err(e) => eprintln!("Listener accept failed: \n{e}"),
            }
        }
    }

    // ========== Get Method ============

    // Get routes without middleware
    pub async fn get<F, Fut>(&self, path: &str, handler: F)
        where
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = ()> + Send + 'static
    {
        // Empty middleware vec
        let empty_middleware: Vec<Arc<Middleware>> = vec![];

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            let fut = handler(req, res);
            Box::pin(fut) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.get_with_middleware_run(path, empty_middleware, wrapped_handler).await;
    }

    // Get routes with middleware
    pub async fn get_with_middleware<Mfut, F, Ffut>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) -> Mfut>,
        handler: F
    )
        where
            Mfut: Future<Output = ()> + Send + 'static,
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Ffut + Send + Sync + 'static,
            Ffut: Future<Output = ()> + Send + 'static
    {
        let wrapped_middleware: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|mw_fn| {
                let wrapped = move |
                    req: Arc<RwLock<Request>>,
                    res: Arc<RwLock<Response>>,
                    next: Next
                | {
                    Box::pin(mw_fn(req, res, next)) as Pin<Box<dyn Future<Output = ()> + Send>>
                };
                Arc::new(wrapped) as Arc<Middleware>
            })
            .collect();

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            Box::pin(handler(req, res)) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.get_with_middleware_run(path, wrapped_middleware, wrapped_handler).await;
    }

    // Responsible for runing both Get without middleware or with middleware
    async fn get_with_middleware_run(
        &self,
        path: &str,
        middleware: Vec<Arc<Middleware>>,
        handler: Arc<Handler>
    ) {
        let route = Route {
            method: "GET".to_string(),
            path: path.to_string(),
            middleware,
            handler,
        };

        self.routes.write().await.push(route);
    }

    // // ========== Post Method ============
    // POST routes without middleware
    pub async fn post<F, Fut>(&self, path: &str, handler: F)
        where
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = ()> + Send + 'static
    {
        let empty_middleware: Vec<Arc<Middleware>> = vec![];

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            let fut = handler(req, res);
            Box::pin(fut) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.post_with_middleware_run(path, empty_middleware, wrapped_handler).await;
    }

    // POST with middleware
    pub async fn post_with_middleware<Mfut, F, Ffut>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) -> Mfut>,
        handler: F
    )
        where
            Mfut: Future<Output = ()> + Send + 'static,
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Ffut + Send + Sync + 'static,
            Ffut: Future<Output = ()> + Send + 'static
    {
        let wrapped_middleware: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|mw_fn| {
                let wrapped = move |
                    req: Arc<RwLock<Request>>,
                    res: Arc<RwLock<Response>>,
                    next: Next
                | {
                    Box::pin(mw_fn(req, res, next)) as Pin<Box<dyn Future<Output = ()> + Send>>
                };
                Arc::new(wrapped) as Arc<Middleware>
            })
            .collect();

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            Box::pin(handler(req, res)) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.post_with_middleware_run(path, wrapped_middleware, wrapped_handler).await;
    }

    // POST route registration helper
    async fn post_with_middleware_run(
        &self,
        path: &str,
        middleware: Vec<Arc<Middleware>>,
        handler: Arc<Handler>
    ) {
        let route = Route {
            method: "POST".to_string(),
            path: path.to_string(),
            middleware,
            handler,
        };

        self.routes.write().await.push(route);
    }

    // // ========== Put Method ============
    // PUT routes without middleware
    pub async fn put<F, Fut>(&self, path: &str, handler: F)
        where
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = ()> + Send + 'static
    {
        let empty_middleware: Vec<Arc<Middleware>> = vec![];

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            let fut = handler(req, res);
            Box::pin(fut) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.put_with_middleware_run(path, empty_middleware, wrapped_handler).await;
    }

    // PUT with middleware
    pub async fn put_with_middleware<Mfut, F, Ffut>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) -> Mfut>,
        handler: F
    )
        where
            Mfut: Future<Output = ()> + Send + 'static,
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Ffut + Send + Sync + 'static,
            Ffut: Future<Output = ()> + Send + 'static
    {
        let wrapped_middleware: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|mw_fn| {
                let wrapped = move |
                    req: Arc<RwLock<Request>>,
                    res: Arc<RwLock<Response>>,
                    next: Next
                | {
                    Box::pin(mw_fn(req, res, next)) as Pin<Box<dyn Future<Output = ()> + Send>>
                };
                Arc::new(wrapped) as Arc<Middleware>
            })
            .collect();

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            Box::pin(handler(req, res)) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.put_with_middleware_run(path, wrapped_middleware, wrapped_handler).await;
    }

    // PUT route registration helper
    async fn put_with_middleware_run(
        &self,
        path: &str,
        middleware: Vec<Arc<Middleware>>,
        handler: Arc<Handler>
    ) {
        let route = Route {
            method: "PUT".to_string(),
            path: path.to_string(),
            middleware,
            handler,
        };

        self.routes.write().await.push(route);
    }

    // // ========== Delete Method ============

    // DELETE routes without middleware
    pub async fn delete<F, Fut>(&self, path: &str, handler: F)
        where
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = ()> + Send + 'static
    {
        let empty_middleware: Vec<Arc<Middleware>> = vec![];

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            let fut = handler(req, res);
            Box::pin(fut) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.delete_with_middleware_run(path, empty_middleware, wrapped_handler).await;
    }

    // DELETE with middleware
    pub async fn delete_with_middleware<Mfut, F, Ffut>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) -> Mfut>,
        handler: F
    )
        where
            Mfut: Future<Output = ()> + Send + 'static,
            F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) -> Ffut + Send + Sync + 'static,
            Ffut: Future<Output = ()> + Send + 'static
    {
        let wrapped_middleware: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|mw_fn| {
                let wrapped = move |
                    req: Arc<RwLock<Request>>,
                    res: Arc<RwLock<Response>>,
                    next: Next
                | {
                    Box::pin(mw_fn(req, res, next)) as Pin<Box<dyn Future<Output = ()> + Send>>
                };
                Arc::new(wrapped) as Arc<Middleware>
            })
            .collect();

        let wrapped_handler: Arc<Handler> = Arc::new(move |req, res| {
            Box::pin(handler(req, res)) as Pin<Box<dyn Future<Output = ()> + Send>>
        });

        self.delete_with_middleware_run(path, wrapped_middleware, wrapped_handler).await;
    }

    // DELETE route registration helper
    async fn delete_with_middleware_run(
        &self,
        path: &str,
        middleware: Vec<Arc<Middleware>>,
        handler: Arc<Handler>
    ) {
        let route = Route {
            method: "DELETE".to_string(),
            path: path.to_string(),
            middleware,
            handler,
        };

        self.routes.write().await.push(route);
    }
}
