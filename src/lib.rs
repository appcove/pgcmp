use std::path::PathBuf;

pub mod cli;
pub mod commands;
pub mod comparison;
pub mod config;
pub mod db;
pub mod git;
pub mod memfs;
pub mod schema;

pub struct App {
    pub path: PathBuf,
}

impl App {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn leak(self) -> &'static Self {
        Box::leak(Box::new(self))
    }
}
