use apacheta::*;
use simplelog::{Config, LevelFilter, TermLogger};
use std::path::Path;

fn main() {
    TermLogger::init(LevelFilter::Info, Config::default()).unwrap();

    let config = read_config(Path::new("config.toml")).unwrap();
    let articles = process_gpx_dir(&config);

    generate_index(&config, articles);
}
