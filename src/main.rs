use apacheta::*;
use simplelog::{Config, LevelFilter, TermLogger};
use std::path::Path;
use tera::*;

fn main() {
    TermLogger::init(LevelFilter::Info, Config::default()).unwrap();

    let config = read_config(Path::new("config.toml")).unwrap();
    let target_dir = Path::new(&config.data.site_output);

    let tera = compile_templates!("site/templates/*");

    let articles = process_gpx_dir(&config);

    let mut index_context = Context::new();
    index_context.add("config", &config);
    index_context.add("static_dir", "static");
    index_context.add("articles", &articles);

    render_html(&tera, index_context, &target_dir, "index", "index.html").unwrap();
}
