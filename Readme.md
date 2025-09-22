# Glote - Rust Multithreading Web Library

Glote is a fast web library in pure Rust, inspired by simplicity and performance. It supports routing, middleware, path/query/body parsing, JSON responses — all thread-safe and scalable using worker pools.

- Note: Don't use version 0.2.4 this one is added a linux base function i fix it in 0.3.0^

# Getting Started

## Create a server

```rust
use glote::Glote;

fn main() {
    let server = Glote::new();
    server.block_on(async{
        run_server(server.clone()).await;
    })
}

async fn run_server(server:Arc<Glote>){
    server.get("/", |req, res| {
            res.send("Hello, Glote!").await;
        }
    ).await;

    server.listen(8080).await;
}
```

## Routing

Glote supports GET, POST, PUT, and DELETE methods.

- GET

```rust
server.get("/hello", |req, res| {
    res.send("GET route").await;
}).await;

// or
use glote::han;

server.get("/hello",han!(req,res,{
    res.send("GET route").await;
}))

```

- POST

```rust
server.post("/submit", |req, res| {
    let data = req.read().await.body.unwrap_or("No body".into());
    res.send(&format!("Posted: {}", data)).await;
}).await;
```

- PUT

```rust
server.put("/update", |req, res| {
    res.send("PUT route").await;
}).await;
```

- DELETE

```rust
server.delete("/delete", |req, res| {
    res.send("DELETE route").await;
}).await;
```

## Path Parameters

Use : to define path variables.

```rust
server.get("/user/:id", |req, res| {
    let user_id = req.read().await.params("id").cloned().unwrap_or_default();
    res.send(&format!("User ID: {}", user_id)).await;
}).await;
```

## Query Parameters

```rust
server.get("/search", |req, res| {
    let query = req.read().await.query("q").unwrap_or("none".into());
    res.send(&format!("You searched: {}", query)).await;
}).await;
```

## Request Body

Supports reading body for POST, PUT, etc.

```rust
server.post("/echo", |req, res| {
    let body = req.read().await.body().unwrap_or_default();
    res.send(&body).await;
}).await;
```

# Middleware

Middlewares can inspect, log, or halt requests before reaching the handler.

## Global Middleware

```rust
use glote::{RequestExt}; // Needed for req.with_read

server.use_middleware(|req, _res, next| {
    req.with_read(|r| {
        let r = r.read().await;
        println!("{} {}", r.method, r.path);
    }).await;
    next().await;
}).await;
```

- or

Make sure you release the lock

```rust
server.use_middleware(|req, _res, next| {
    let req = req.read().await.unwrap();
    println!("{} {}", req.method, req.path);
    drop(req); // Drop it manually or it's takes resources or wirte lock which makes trouble in some cases
    next().await;
}).await;
```

## Route-specific Middleware

```rust
use glote::{Req,Res,Next,mid};
use std::sync::{Arc, RwLock};

// Req = Arc<Rwlock<Request>>
// Res = Arc<Rwlock<Responce>>
// Next = &mut dyn FnMut()

let logger = mid!(req, res, next, {
        println!("1. {}", req.read().await.path);

        next().await;
    });



server.get_with_middleware("/check", vec![logger], |req, res| {
    res.send("Checked with middleware").await;
});
```

## Stop Middleware Chain

Return from middleware:

```rust
server.use_middleware(|_req, res, _next| {
    res.status(401).await;
    res.send("Unauthorized").await; // send and json method will automatically stop chain
});

```

# CORS Middleware

Glote supports pluggable CORS middleware to control cross-origin requests. You can use the built-in Cors struct to allow or deny specific origins.

```rust
use glote::{Cors, CorsExt};
// Allow only specific origins (use "*" to allow all)
let cors = Cors::new(&["http://localhost:4000", "http://127.0.0.1:4000"]);

// Register CORS middleware
server.use_middleware(move |req, res, next| {
        let cors = Arc::clone(&cors);
        async move {
            cors.run_middleware(req, res, next).await;
        }
    }
).await;
```

# Static file serve

If you set static path the defualt root / is index.html

```rust
server.static_path("public").await; // Path of you static files
```

# Response Extensions

## Text Response

```rust
res.send("Hello World!").await;
```

## JSON Response

```rust
#[derive(Serialize)]
struct Message {
    msg: String,
}

res.json(&Message { msg: "Hi".into() }).await;
```

## Set Status

```rust
res.status(201).await; // Created
```

# Example App

```rust
fn main() {
    let server = Glote::new();

    server.block_on(async{
        run_server(server.clone()).await;
    });
}

async run_server(server:Arc<Glote>){
    server.use_middleware(|req, _res, next| {
        req.with_read(|r| {
            println!("Global: {} {}", r.method, r.path);
        }).await;
        next().await;
    });

    server.get("/hello/:name", |req, res| {
        let name = req.params("name").unwrap_or("guest".into());
        res.send(&format!("Hello, {}!", name)).await;
    });

    server.listen(3000).await;
}

```

# Request Struct

```rs
pub struct Request {
    pub method: String,
    pub path: String,
    pub path_params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: Option<String>,
    pub headers: HashMap<String, String>,
}
```

# RequestExt Trait

Highly recommend to use this trait in middleware

```rust
pub trait RequestExt {
    fn with_write<F>(&self, f: F) where F: FnOnce(&mut Request);
    fn with_read<F, R>(&self, f: F) -> R where F: FnOnce(&Request) -> R;

    fn path(&self) -> Option<String>;
    fn query(&self, key: &str) -> Option<String>;
    fn params(&self, key: &str) -> Option<String>;
    fn body(&self) -> Option<String>;
}
```

## Trait Methods

Highly recommend to use this trait in middleware

- with_read(&self, f: F) -> R

Reads from the request immutably inside a closure.

```rust
use glote::{RequestExt};

req.with_read(|r| {
println!("Method: {}", r.method);
}).await;
```

- with_write(&self, f: F)

Mutably modifies the request inside a closure.

```rust
use glote::{RequestExt};

req.with_write(|r| {
    r.write().await.path_params.insert("user".to_string(), "123".to_string());
}).await;
```

# Response Struct

```rs
pub struct Response {
    stream: Arc<RwLock<TcpStream>>,
    status: u16,
    headers: Arc<RwLock<HashMap<String, String>>>,
    stopped: Arc<RwLock<bool>>,
}
```

# ResponseExt Trait

```rust
pub trait ResponseExt {
    fn status(&self, code: u16);
    fn send(&self, body: &str);
    fn json<T: Serialize>(&self, data: &T);
}
```

```rust
use glote::{ ResponseExt };

res.status(200).await;
res.send("OK").await;

res.json(&serde_json::json!({ "message": "Success" })).await;
```

# Feature Roadmap

    ✅ Middleware (global and route)

    ✅ Path/query/body parsing

    ✅ JSON and plain response

    ✅ Route match + param parsing

    ✅ Workerpool

    ✅ Static file serving

    ⏳ Cookie/session support

    ⏳ TLS support
