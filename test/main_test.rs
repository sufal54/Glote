use std::sync::{ Arc, RwLock };
use glote::{ Glote, Request, Response }; // Replace with your actual module structure

#[test]
fn test_server_instantiation() {
    let server = Glote::new();
    assert!(Arc::strong_count(&server) >= 1);
}

#[test]
fn test_set_workers() {
    let mut server = Glote::new();
    Arc::get_mut(&mut server).unwrap().set_warkers(8); // only works if Arc has only 1 strong ref
}
