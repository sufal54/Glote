use std::io::{ BufRead, BufReader, ErrorKind, Read };
use std::net::TcpListener;
use std::sync::{ Arc, RwLock };
use std::time::Instant;

use crate::request::{ parse_path_params, Request };
use crate::response::Response;
use crate::workerpool::WorkerPool;

// Atomically reference counter with safely travale though thread with own lifetime/ownership
pub type Next<'a> = &'a mut dyn FnMut();

pub type Handler = dyn Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static;
pub type Middleware = dyn Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) +
    Send +
    Sync +
    'static;

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
    pool: WorkerPool,
    static_path: Option<String>,
}

impl Glote {
    // Returns Arc self
    pub fn new() -> Arc<Self> {
        // Number of core in our cpu
        let num_cores = std::thread
            ::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        // Total worker Defualt (total core * 4) or 4
        Arc::new(Self {
            routes: Arc::new(RwLock::new(Vec::new())),
            middleware: Arc::new(RwLock::new(Vec::new())),
            pool: WorkerPool::new(num_cores * 4),
            static_path: None,
        })
    }
    // Manually set number of workers
    pub fn set_warkers(&mut self, size: usize) {
        self.pool = WorkerPool::new(size);
    }

    // Runs Global+route middleware and final handler
    fn run_handlers(
        &self,
        req: Arc<RwLock<Request>>,
        res: Arc<RwLock<Response>>,
        middlewares: &[Arc<Middleware>],
        final_handler: impl FnMut() + Send + 'static
    ) {
        let req = Arc::clone(&req);
        let res = Arc::clone(&res);
        let middlewares = middlewares.to_vec();

        // Store final handler into box for linklist fashion with multiple middlewares
        let mut final_handler = Some(Box::new(final_handler) as Box<dyn FnMut()>);
        // makes chain in reverse way
        for mw in middlewares.into_iter().rev() {
            let req = req.clone();
            let res = res.clone();
            let mut next = final_handler.take().unwrap();
            final_handler = Some(
                Box::new(move || {
                    // case we send response and don't want to go deeper
                    if !res.read().unwrap().is_stopped() {
                        mw(req.clone(), res.clone(), &mut next);
                    }
                })
            );
        }

        if let Some(mut chain) = final_handler {
            // case we send response and don't want to go deeper
            if !res.read().unwrap().is_stopped() {
                chain();
            }
        }
    }

    // Set Global Middleware
    pub fn use_middleware<F>(&self, middleware: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next) + Send + Sync + 'static
    {
        let mut middlewares = self.middleware.write().unwrap();
        middlewares.push(Arc::new(middleware));
    }

    pub fn static_file(&mut self, path: &str) {
        self.static_path = Some(path.into());
    }

    /**
     * Start our server at specific port
     */
    pub fn listen(self: Arc<Self>, port: u16) {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        listener.set_nonblocking(true).unwrap();

        println!("\n---------------------\nServer running on port {}", port);

        // Listening incoming request
        loop {
            match listener.accept() {
                Ok((s, _add)) => {
                    // Filter out raw stream from inconging request
                    s.set_nonblocking(true).unwrap();
                    let stream = s;
                    // Clone of our Routes
                    let routers_clone = {
                        let guard = self.routes.read().unwrap();
                        guard.clone()
                    };
                    // Clone of our Middleware
                    let middleware_clone = {
                        let guard = self.middleware.read().unwrap();
                        guard.clone()
                    };
                    // static file not used
                    let _static_file = self.static_path.clone();

                    let this = self.clone();
                    // Assign a Worker though warkerpool
                    self.pool.execute(move || {
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
                            match reader.read_line(&mut buffer) {
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
                                    std::thread::sleep(std::time::Duration::from_millis(5));
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
                            match reader.read_exact(&mut buf) {
                                Ok(_) => {
                                    let body = String::from_utf8_lossy(&buf).to_string();
                                    body_lines.extend(body.lines().map(|s| s.to_string()));
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    std::thread::sleep(std::time::Duration::from_millis(5));
                                    // Optionally retry loop here (not necessary unless needed)
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
                                    let combined_middleware: Vec<_> = middleware_clone
                                        .iter()
                                        .chain(route.middleware.iter())
                                        .cloned()
                                        .collect();

                                    if let Some(res_actual) = res_opt.take() {
                                        // Move ownership
                                        let req_for_handler = Arc::clone(&req_with_params);
                                        let res_for_handler = Arc::clone(&res_actual);
                                        // Call run_handler
                                        this.run_handlers(
                                            Arc::clone(&req_for_handler),
                                            Arc::clone(&res_for_handler),
                                            &combined_middleware,
                                            {
                                                let req_inner = req_for_handler.clone();
                                                let res_inner = res_for_handler.clone();
                                                move || {
                                                    (route.handler)(
                                                        req_inner.clone(),
                                                        res_inner.clone()
                                                    );
                                                }
                                            }
                                        );

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
                                let mut res = res.write().unwrap();
                                res.status(404);
                                res.send("404 Not Found");
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
    pub fn get<F>(&self, path: &str, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Make empty middleware
        let empty_middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)> = vec![];

        self.get_with_middleware(path, empty_middleware, handler);
    }

    // Get routes with middleware
    pub fn get_with_middleware<F>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)>,
        handler: F
    )
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Warp all middleware with Arc
        let wrapped: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|m| Arc::new(m) as Arc<Middleware>)
            .collect();

        self.get_with_middleware_run(path, wrapped, handler);
    }

    // Responsible for runing both Get without middleware or with middleware
    fn get_with_middleware_run<F>(&self, path: &str, middleware: Vec<Arc<Middleware>>, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Sets route Metadata
        let route = Route {
            method: "GET".to_string(),
            path: path.to_string(),
            middleware,
            handler: Arc::new(handler),
        };
        // Push with Global routes
        self.routes.write().unwrap().push(route);
    }

    // ========== Post Method ============
    // Post routes without middleware
    pub fn post<F>(&self, path: &str, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Make empty middleware
        let empty_middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)> = vec![];

        self.post_with_middleware(path, empty_middleware, handler);
    }

    // Post routes with middleware
    pub fn post_with_middleware<F>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)>,
        handler: F
    )
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Warp all middleware with Arc
        let wrapped: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|m| Arc::new(m) as Arc<Middleware>)
            .collect();

        self.post_with_middleware_run(path, wrapped, handler);
    }

    // Responsible for running both Post without middleware or with middleware
    fn post_with_middleware_run<F>(&self, path: &str, middleware: Vec<Arc<Middleware>>, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        let route = Route {
            method: "POST".to_string(),
            path: path.to_string(),
            middleware,
            handler: Arc::new(handler),
        };
        self.routes.write().unwrap().push(route);
    }

    // ========== Put Method ============
    // Put routes without middleware
    pub fn put<F>(&self, path: &str, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Make empty middleware
        let empty_middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)> = vec![];

        self.put_with_middleware(path, empty_middleware, handler);
    }

    // Put routes with middleware
    pub fn put_with_middleware<F>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)>,
        handler: F
    )
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Warp all middleware with Arc
        let wrapped: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|m| Arc::new(m) as Arc<Middleware>)
            .collect();

        self.put_with_middleware_run(path, wrapped, handler);
    }

    // Responsible for running both Put without middleware or with middleware
    fn put_with_middleware_run<F>(&self, path: &str, middleware: Vec<Arc<Middleware>>, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        let route = Route {
            method: "PUT".to_string(),
            path: path.to_string(),
            middleware,
            handler: Arc::new(handler),
        };
        self.routes.write().unwrap().push(route);
    }

    // ========== Delete Method ============

    // Delete routes without middleware
    pub fn delete<F>(&self, path: &str, handler: F)
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Make empty middleware
        let empty_middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)> = vec![];

        self.delete_with_middleware(path, empty_middleware, handler);
    }

    // Delete routes with middleware
    pub fn delete_with_middleware<F>(
        &self,
        path: &str,
        middleware: Vec<fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>, Next)>,
        handler: F
    )
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        // Warp all middleware with Arc
        let wrapped: Vec<Arc<Middleware>> = middleware
            .into_iter()
            .map(|m| Arc::new(m) as Arc<Middleware>)
            .collect();

        self.delete_with_middleware_run(path, wrapped, handler);
    }

    // Responsible for running both Delete without middleware or with middleware
    fn delete_with_middleware_run<F>(
        &self,
        path: &str,
        middleware: Vec<Arc<Middleware>>,
        handler: F
    )
        where F: Fn(Arc<RwLock<Request>>, Arc<RwLock<Response>>) + Send + Sync + 'static
    {
        let route = Route {
            method: "DELETE".to_string(),
            path: path.to_string(),
            middleware,
            handler: Arc::new(handler),
        };
        self.routes.write().unwrap().push(route);
    }
}
