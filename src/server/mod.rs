use std::collections::HashMap;
use std::io::{ BufRead, BufReader, Error, ErrorKind, Read };
use std::net::TcpListener;
use std::os::fd::{ AsRawFd, RawFd };
use std::ptr;
use std::sync::{ Arc, RwLock };
use std::time::Instant;
use libc::*;

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

        // Create epoll
        let epfd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if epfd == -1 {
            panic!("epoll_create1 failed");
        }
        // Tcp_listener FD
        let listener_fd = listener.as_raw_fd();
        // Create event
        let mut ev = epoll_event {
            events: EPOLLIN as u32,
            u64: listener_fd as u64,
        };
        unsafe {
            epoll_ctl(epfd, EPOLL_CTL_ADD, listener_fd, &mut ev);
        }

        let mut clients: HashMap<RawFd, std::net::TcpStream> = HashMap::new();
        let mut buffers: HashMap<RawFd, Vec<u8>> = HashMap::new();

        let num_cores = std::thread
            ::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        // Events size cpu_cores * 1024
        let mut events = vec![epoll_event { events: 0, u64: 0 }; num_cores*1024];

        println!("\n---------------------\nServer running on port {}", port);

        loop {
            // Wait for Notify FD
            let nfds = unsafe { epoll_wait(epfd, events.as_mut_ptr(), events.len() as i32, -1) };

            if nfds < 0 {
                continue;
            }

            // Loop into all sockets we are reciveing
            for i in 0..nfds as usize {
                // Get the raw fd
                let fd = events[i].u64 as RawFd;
                // If it incoming request
                if fd == listener_fd {
                    // Listening incoming request
                    loop {
                        match listener.accept() {
                            Ok((stream, _addr)) => {
                                // Stream non_blocking
                                stream.set_nonblocking(true).unwrap();
                                // Get stread fd
                                let cfd = stream.as_raw_fd();
                                // Notify when its ready
                                let mut ev = epoll_event {
                                    events: EPOLLIN as u32,
                                    u64: cfd as u64,
                                };

                                // epoll_wait will return an event whene this client data ready
                                unsafe {
                                    epoll_ctl(epfd, EPOLL_CTL_ADD, cfd, &mut ev);
                                }

                                // Store the client in clients hashmap
                                clients.insert(cfd, stream);
                                // Empty buffer for the client
                                buffers.insert(cfd, Vec::new());
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                break;
                            }
                            Err(e) => {
                                eprintln!("accept error: {}", e);
                                break;
                            }
                        }
                    }
                    continue;
                }

                // client socket ready
                let mut remove = false;

                if let Some(stream) = clients.get_mut(&fd) {
                    // Clinet buffer
                    let mut tmp = [0u8; 4096];
                    match stream.read(&mut tmp) {
                        Ok(0) => {
                            // No data client closed
                            remove = true;
                        }
                        Ok(n) => {
                            // Store data into client buffer Hashmaps
                            let entry = buffers.get_mut(&fd).unwrap();
                            entry.extend_from_slice(&tmp[..n]);

                            // check if we have full headers
                            if
                                let Some(headers_end_pos) = entry
                                    .windows(4)
                                    .position(|w| w == b"\r\n\r\n")
                            {
                                // Stroe contents
                                let headers = &entry[..headers_end_pos + 4];
                                let headers_str = String::from_utf8_lossy(headers);
                                let mut content_length: usize = 0;
                                for line in headers_str.lines() {
                                    if line.to_ascii_lowercase().starts_with("content-length:") {
                                        if let Some(v) = line.split(':').nth(1) {
                                            if let Ok(len) = v.trim().parse::<usize>() {
                                                content_length = len;
                                            }
                                        }
                                    }
                                }

                                let total_len = headers_end_pos + 4 + content_length; // Request total length
                                // Check raw data greater or equal to total length
                                if entry.len() >= total_len {
                                    // Request in rwa form
                                    let request_bytes = entry
                                        .drain(..total_len)
                                        .collect::<Vec<u8>>();

                                    // remove client from epoll and our maps
                                    unsafe {
                                        epoll_ctl(epfd, EPOLL_CTL_DEL, fd, ptr::null_mut());
                                    }
                                    // Delete client and get it
                                    let stream = clients.remove(&fd).unwrap();
                                    buffers.remove(&fd);

                                    // parse request headers only
                                    let headers_only = &request_bytes[..headers_end_pos + 4];
                                    let mut lines: Vec<String> = Vec::new();
                                    for line in headers_only.split(|&b| b == b'\n') {
                                        // trim trailing \r and whitespace
                                        let s = String::from_utf8_lossy(line)
                                            .trim_end_matches('\r')
                                            .to_string();
                                        lines.push(s);
                                    }
                                    // Add body lines
                                    if content_length > 0 {
                                        // Raw body
                                        let body_bytes =
                                            &request_bytes
                                                [
                                                    headers_end_pos + 4..headers_end_pos +
                                                        4 +
                                                        content_length
                                                ];
                                        // Parse into string
                                        let body_str =
                                            String::from_utf8_lossy(body_bytes).to_string();
                                        // Adds a empty string to seprate header and body
                                        lines.push(String::new());
                                        lines.extend(body_str.lines().map(|s| s.to_string()));
                                    } else {
                                        // There is no body just push empty string
                                        lines.push(String::new());
                                    }

                                    // clone routes/middleware once for worker
                                    let routers_clone = {
                                        let guard = self.routes.read().unwrap();
                                        guard.clone()
                                    };
                                    let middleware_clone = {
                                        let guard = self.middleware.read().unwrap();
                                        guard.clone()
                                    };
                                    let this = self.clone();

                                    // Spawn worker with parsed request (no further reading)
                                    self.pool.execute(move || {
                                        let now = Instant::now();
                                        // Parse metadata into Request struct
                                        let req = Request::new(&lines);
                                        // Parse stream into Response struct
                                        let mut res_opt = Some(
                                            Arc::new(RwLock::new(Response::new(stream)))
                                        );

                                        // Check is Route have or not
                                        let mut matched = false;
                                        // Iterate in Routes
                                        for route in routers_clone.into_iter() {
                                            // Case method same
                                            if route.method == req.method {
                                                // Parse params
                                                if
                                                    let Some(params) = parse_path_params(
                                                        &route.path,
                                                        &req.path
                                                    )
                                                {
                                                    // CLone req inside have params
                                                    let mut req_with_params = req.clone();
                                                    req_with_params.path_params = params;
                                                    let req_with_params = Arc::new(
                                                        RwLock::new(req_with_params)
                                                    );
                                                    // Combined Global Middleware and Routes Middleware
                                                    let combined_middleware: Vec<_> =
                                                        middleware_clone
                                                            .iter()
                                                            .chain(route.middleware.iter())
                                                            .cloned()
                                                            .collect();

                                                    if let Some(res_actual) = res_opt.take() {
                                                        // Take arc clone
                                                        let req_for_handler = Arc::clone(
                                                            &req_with_params
                                                        );
                                                        let res_for_handler = Arc::clone(
                                                            &res_actual
                                                        );
                                                        // Call run_handler
                                                        this.run_handlers(
                                                            Arc::clone(&req_for_handler),
                                                            Arc::clone(&res_for_handler),
                                                            &combined_middleware,
                                                            {
                                                                let req_inner =
                                                                    req_for_handler.clone();
                                                                let res_inner =
                                                                    res_for_handler.clone();
                                                                move || {
                                                                    (route.handler)(
                                                                        req_inner.clone(),
                                                                        res_inner.clone()
                                                                    );
                                                                }
                                                            }
                                                        );
                                                        // Route matched
                                                        matched = true;
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        // Duration to fullfill the request
                                        let duration = now.elapsed();
                                        if !matched {
                                            if let Some(res) = res_opt {
                                                let mut res = res.write().unwrap();
                                                res.status(404);
                                                res.send("404 Not Found");
                                            }
                                            println!(
                                                "\x1b[31m{} {}: {:?}\x1b[0m ",
                                                req.method,
                                                req.path,
                                                duration
                                            );
                                        } else {
                                            println!(
                                                "\x1b[32m{} {}: {:?}\x1b[0m ",
                                                req.method,
                                                req.path,
                                                duration
                                            );
                                        }
                                    });
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            // nothing to do
                        }
                        Err(e) => {
                            eprintln!("read error fd {}: {}", fd, e);
                            remove = true;
                        }
                    }
                }
                // Remove it from event also client and buffer
                if remove {
                    unsafe {
                        epoll_ctl(epfd, EPOLL_CTL_DEL, fd, ptr::null_mut());
                    }
                    clients.remove(&fd);
                    buffers.remove(&fd);
                }
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
