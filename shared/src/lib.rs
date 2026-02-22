extern crate core;

pub mod auth;
pub mod config;
pub mod db;
pub mod errors;

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
