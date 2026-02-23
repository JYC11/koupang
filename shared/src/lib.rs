extern crate core;

pub mod auth;
pub mod cache;
pub mod config;
pub mod db;
pub mod dto_helpers;
pub mod email;
pub mod errors;
pub mod grpc;
pub mod health;
pub mod observability;
pub mod responses;
pub mod server;

#[derive(Clone)]
pub struct CommonAppState {
    pub port: u16,
}

impl CommonAppState {
    pub fn new() -> Self {
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse()
            .expect("PORT must be a valid u16");

        Self { port }
    }
}
