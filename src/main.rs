use apacheta::*;
use log::*;
use simplelog::{Config, LevelFilter, TermLogger};
use std::fs;
use std::path::Path;
use tera::*;

fn main() {
    TermLogger::init(LevelFilter::Info, Config::default()).unwrap();

    let config = read_config(Path::new("config.toml")).unwrap();
    let gpx_dir = Path::new(&config.data.gpx_input);
    let target_dir = Path::new(&config.data.site_output);

    let mut articles: Vec<TrackArticle> = Vec::new();

    let tera = compile_templates!("site/templates/*");

    for entry in fs::read_dir(gpx_dir).unwrap() {
        let gpx_path = entry.unwrap().path();
        if gpx_path.extension().unwrap() == "gpx" {
            info!("Processing {}", gpx_path.display());
            match gpx_to_html(&gpx_path, target_dir, &tera, &config) {
                Some(article) => articles.push(article),
                None => continue,
            }
        }
    }

    let mut index_context = Context::new();
    index_context.add("config", &config);
    index_context.add("static_dir", "static");
    index_context.add("articles", &articles);

    render_html(&tera, index_context, &target_dir, "index", "index.html").unwrap();
}
