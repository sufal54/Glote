use std::sync::{ Arc, RwLock };
use glote::{ Cors, Glote, Request, Response, ResponseExt, CorsExt };

#[test]
fn test_server_instantiation() {
    let server = Glote::new();
    assert!(Arc::strong_count(&server) >= 1);
}

#[test]
fn test_set_workers() {
    let mut server = Glote::new();
    Arc::get_mut(&mut server).unwrap().set_warkers(8);
}

fn test_server_working() {
    let mut server = Glote::new();
    let cors = Cors::new(&["http://localhost:4000"]);

    server.use_middleware({
        let cors = Arc::clone(&cors);
        move |req, res, next| {
            cors.run_middleware(req, res, next);
        }
    });

    server.get("/", |req, res| {
        res.status(200);
        res.send("okay");
    });

    server.listen(3000);
}
