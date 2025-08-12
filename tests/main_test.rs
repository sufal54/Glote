use std::{ sync::{ Arc }, thread, time::Duration };
use glote::{ Glote, ResponseExt };

#[test]
fn test_server_instantiation() {
    let server = Glote::new();
    assert!(Arc::strong_count(&server) >= 1);
}
/*
use std::sync::Arc;
use glote::{ Cors, CorsExt, Glote, Next, Req, RequestExt, Res, ResponseExt, mid, han };

async fn hello(server: Arc<Glote>) {
    let cors = Cors::new(&vec!["127.0.0.1:5000"]);

    // let cors_clone = Arc::clone(&cors);
    server.use_middleware(move |req, res, next| {
        let cors = Arc::clone(&cors);
        async move {
            cors.run_middleware(req, res, next).await;
        }
    }).await;

    // Global middleware logs every request path
    server.use_middleware(|req, res, next| async move {
        req.with_write(|req| async move {
            req.write().await.path = "/hh".into();
        }).await;
        next().await;
    }).await;

    // GET route with middleware
    server.get_with_middleware(
        "/",
        vec![
            mid!(req, res, next, {
                println!("1. {}", req.read().await.path);
                next().await;
            }),
            mid!(req, res, next, {
                println!("2. Another middleware");
                next().await;
            })
        ],
        han!(req, res, {
            println!("Handler");
            res.status(200).await;
            res.send("okay").await;
        })
    ).await;

    let yo = mid!(req, res, next, {
        println!("1. {}", req.read().await.path);

        next().await;
    });

    server.post_with_middleware(
        "/",
        vec![
            yo,
            mid!(req, res, next, {
                println!("2. Another middleware");
                next().await;
            })
        ],
        han!(req, res, {
            println!("Handler");
            res.status(200).await;
            res.send("okay").await;
        })
    ).await;

    // Simple GET with path param and query param
    server.get("/hello/:name", |req, res| async move {
        let binding = req.read().await;
        let name = binding
            .params("name")
            .cloned()
            .unwrap_or_else(|| "stranger".to_string());
        let q = binding.query("q").cloned().unwrap_or_default();

        let message = format!("Hello, {}! Query: {}", name, q);
        res.write().await.send(&message).await;
    }).await;

    // POST route echoes request body
    server.post("/echo", |req, res| async move {
        let body = req.read().await.body.clone().unwrap_or_default();
        res.write().await.send(&format!("POST Echo: {}", body)).await;
    }).await;

    // PUT route example
    server.put("/update", |req, res| async move {
        let body = req.read().await.body.clone().unwrap_or_default();
        res.write().await.send(&format!("PUT Received: {}", body)).await;
    }).await;

    // DELETE route example
    server.delete("/remove/:id", |req, res| async move {
        let id = req
            .read().await
            .params("id")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        res.write().await.send(&format!("DELETE Requested for ID: {}", id)).await;
    }).await;

    server.listen(3000).await.unwrap();
}
fn main() {
    let server = Glote::new();
    server.block_on(async {
        hello(server.clone()).await;
    });
}

*/

// #[test]
// fn test_set_workers() {
//     let mut server = Glote::new();
//     Arc::get_mut(&mut server).unwrap().set_warkers(8);
// }

// #[test]
// fn _test_server_working() {
//     let server = Glote::new();

//     server.block_on(fut)
// let cors = Cors::new(&["http://localhost:4000"]);

// server.use_middleware({
//     let cors = Arc::clone(&cors);
//     move |req, res, next| {
//         cors.run_middleware(req, res, next);
//     }
// });
// }

// async fn run(server: Glote) {
//     server.use_middleware(|req, res, next| async {
//         println!("Global middleware");
//         next();
//     });

//     server.get("/", |_req, res| {
//         std::thread::sleep(std::time::Duration::from_millis(500));
//         res.status(200);
//         res.send("okay");
//     });

//     server.listen(3000).await;
// }
