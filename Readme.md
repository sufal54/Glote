# Glote - Rust Web Framework

Glote is a fast web framework in pure Rust, inspired by simplicity and performance. It supports routing, middleware, path/query/body parsing, JSON responses — all thread-safe and scalable using worker pools.

# Getting Started

## Create a server

```rust
use glote::Glote;

fn main() {
    let server = Glote::new();
    server.get("/", |req, res| {
        res.send("Hello, Glote!");
    });

    server.listen(8080);
}
```

## Routing

Glote supports GET, POST, PUT, and DELETE methods.

- GET

```rust
server.get("/hello", |req, res| {
    res.send("GET route");
});
```

- POST

```rust
server.post("/submit", |req, res| {
    let data = req.body().unwrap_or("No body".into());
    res.send(&format!("Posted: {}", data));
});
```

- PUT

```rust
server.put("/update", |req, res| {
    res.send("PUT route");
});
```

- DELETE

```rust
server.delete("/delete", |req, res| {
    res.send("DELETE route");
});
```

## Path Parameters

Use : to define path variables.

```rust
server.get("/user/:id", |req, res| {
    let user_id = req.params("id").unwrap_or_default();
    res.send(&format!("User ID: {}", user_id));
});
```

## Query Parameters

```rust
server.get("/search", |req, res| {
    let query = req.query("q").unwrap_or("none".into());
    res.send(&format!("You searched: {}", query));
});
```

## Request Body

Supports reading body for POST, PUT, etc.

```rust
server.post("/echo", |req, res| {
    let body = req.body().unwrap_or_default();
    res.send(&body);
});
```

# Middleware

Middlewares can inspect, log, or halt requests before reaching the handler.

## Global Middleware

```rust
server.use_middleware(|req, _res, next| {
    req.with_read(|r| {
        println!("{} {}", r.method, r.path);
    });
    next();
});
```

## Route-specific Middleware

```rust
use glote::{Request,Response};
use std::sync::{Arc, RwLock};

fn logger(req: Arc<RwLock<Request>>, res: Arc<RwLock<Response>>, next: &mut dyn FnMut()) {
    println!("[Route MW] {}", req.read().unwrap().path);
    next();
}

server.get_with_middleware("/check", vec![logger], |req, res| {
    res.send("Checked with middleware");
});
```

## Stop Middleware Chain

Use res.stop() to halt propagation:

```rust
server.use_middleware(|_req, res, _next| {
    res.status(401);
    res.send("Unauthorized");
    // Does not call next(), so route handler will not run
});

```

# Response Extensions

## Text Response

```rust
res.send("Hello World!");
```

## JSON Response

```rust
#[derive(Serialize)]
struct Message {
    msg: String,
}

res.json(&Message { msg: "Hi".into() });
```

## Set Status

```rust
res.status(201); // Created
```

# Worker Pool

The framework uses a thread pool (4 × CPU cores by default). You can configure manually:

```rust
let mut server = Glote::new();
server.set_warkers(8); // Manually set worker
```

# Example App

```rust
fn main() {
    let server = Glote::new();

    server.use_middleware(|req, _res, next| {
        req.with_read(|r| {
            println!("Global: {} {}", r.method, r.path);
        });
        next();
    });

    server.get("/hello/:name", |req, res| {
        let name = req.params("name").unwrap_or("guest".into());
        res.send(&format!("Hello, {}!", name));
    });

    server.listen(3000);
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
});
```

- with_write(&self, f: F)

Mutably modifies the request inside a closure.

```rust
use glote::{RequestExt};

req.with_write(|r| {
    r.path_params.insert("user".to_string(), "123".to_string());
});
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

res.status(200);
res.send("OK");

res.json(&serde_json::json!({ "message": "Success" }));
```

# Feature Roadmap

    ✅ Middleware (global and route)

    ✅ Path/query/body parsing

    ✅ JSON and plain response

    ✅ Route match + param parsing

    ⏳ Static file serving

    ⏳ Cookie/session support

    ⏳ TLS support
