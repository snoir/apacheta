use chrono::{DateTime, NaiveDateTime, Utc};
use gexiv2_sys;
use gpx::read;
use gpx::TrackSegment;
use gpx::{Gpx, Track};
use log::*;
use regex::Regex;
use reqwest::Url;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error;
use std::fs;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use tera::{compile_templates, Context, Tera};

#[derive(Serialize, Deserialize)]
pub struct Coordinate {
    lon: f64,
    lat: f64,
}

pub struct Photo {
    path: PathBuf,
    datetime: NaiveDateTime,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub site: Site,
    pub data: Data,
}

#[derive(Serialize, Deserialize)]
pub struct Site {
    pub base_uri: String,
    pub name: String,
    pub proto: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub struct Data {
    pub gpx_input: String,
    pub img_input: String,
    pub site_output: String,
}

#[derive(Serialize, Deserialize)]
pub struct TrackArticle {
    pub title: String,
    pub underscored_title: String,
    pub photos_number: usize,
    pub country: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub coordinate_avg: Coordinate,
}

#[derive(Serialize, Deserialize)]
pub struct ReverseGeocoding {
    pub address: HashMap<String, String>,
}

pub fn read_config(file: &Path) -> Result<Config, io::Error> {
    let mut config_file = File::open(file)?;
    let mut config_str = String::new();
    config_file.read_to_string(&mut config_str)?;

    // Not sure about that, maybe I should use a Box<Error> ?
    match toml::from_str(&config_str) {
        Ok(config) => Ok(config),
        Err(error) => Err(Error::new(ErrorKind::Interrupted, error)),
    }
}

pub fn process_gpx_dir(config: &Config) -> Vec<TrackArticle> {
    let gpx_dir = Path::new(&config.data.gpx_input);
    let target_dir = Path::new(&config.data.site_output);

    let mut articles: Vec<TrackArticle> = Vec::new();

    let tera = compile_templates!("site/templates/*");
    let img_input_dir = Path::new(&config.data.img_input);
    let photo_all = parse_photos(img_input_dir);

    for entry in fs::read_dir(gpx_dir).unwrap() {
        let gpx_path = entry.unwrap().path();
        if gpx_path.extension().unwrap() == "gpx" {
            info!("Processing {}", gpx_path.display());
            match generate_article(&gpx_path, target_dir, &tera, &config, &photo_all) {
                Some(article) => articles.push(article),
                None => continue,
            }
        }
    }

    articles.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    articles
}

pub fn article_gpx_info(gpx_file: &Path) -> (TrackArticle, Vec<Coordinate>) {
    let file = File::open(&gpx_file).unwrap();
    let reader = BufReader::new(file);

    let gpx: Gpx = read(reader).unwrap();
    let track: &Track = &gpx.tracks[0];
    let segment: &TrackSegment = &track.segments[0];

    let mut track_coordinates: Vec<Coordinate> = Vec::new();
    for s in segment.points.iter() {
        track_coordinates.push(Coordinate {
            lon: s.point().x(),
            lat: s.point().y(),
        });
    }

    // type annotations required: cannot resolve `_: std::iter::Sum<f64>` error
    // is generated if avg calculation done in one time, I don't known to fix it
    // for now
    let mut lon_avg: f64 = track_coordinates.iter().map(|x| x.lon).sum();
    lon_avg = lon_avg / track_coordinates.len() as f64;

    let mut lat_avg: f64 = track_coordinates.iter().map(|x| x.lat).sum();
    lat_avg = lat_avg / track_coordinates.len() as f64;

    let coordinate_avg: Coordinate = Coordinate {
        lon: lon_avg,
        lat: lat_avg,
    };

    let start_time = segment.points.first().unwrap().time.unwrap();
    let end_time = segment.points.last().unwrap().time.unwrap();

    let article_title = match gpx.metadata.unwrap().name {
        Some(name) => name,
        None => gpx_file.file_stem().unwrap().to_str().unwrap().to_string(),
    };

    let special_chars_re = Regex::new(r"( |/|\|<|>)").unwrap();
    let article_underscored_title = special_chars_re
        .replace_all(&article_title, "_")
        .to_string();

    (
        TrackArticle {
            title: article_title,
            underscored_title: article_underscored_title,
            photos_number: 0,
            country: String::new(),
            start_time: start_time,
            end_time: end_time,
            coordinate_avg: coordinate_avg,
        },
        track_coordinates,
    )
}

pub fn generate_article(
    gpx_file: &Path,
    target_dir: &Path,
    tera: &Tera,
    config: &Config,
    photo_list: &Vec<Photo>,
) -> Option<TrackArticle> {
    let (article_info, track_coordinates) = article_gpx_info(gpx_file);

    let photo_article = find_photos(photo_list, article_info.start_time, article_info.end_time);
    let mut copied_photos: Vec<String> = Vec::new();
    let photo_target_dir = target_dir
        .join("static/photos")
        .join(article_info.underscored_title.to_string());
    let photo_target_dir_relative =
        Path::new("static/photos").join(article_info.underscored_title.to_string());

    match photo_article {
        Some(photo_article) => {
            let photos = photo_article;

            fs::create_dir_all(&photo_target_dir).unwrap();
            fs::create_dir_all(&photo_target_dir.join("thumbnails")).unwrap();

            for (i, p) in photos.iter().enumerate() {
                let extension = p.path.extension().unwrap().to_str().unwrap();
                let photo_target_file = photo_target_dir.join(format!("{}.{}", i + 1, extension));

                match fs::copy(Path::new(&p.path), &photo_target_file) {
                    Ok(file) => file,
                    Err(error) => {
                        error!("unable to copy {}: {}", &p.path.display(), error);
                        continue;
                    }
                };

                let img = image::open(&Path::new(&photo_target_file))
                    .ok()
                    .expect("Opening image failed");
                let thumbnail = img.thumbnail(300, 300);

                thumbnail
                    .save(&photo_target_dir.join("thumbnails").join(format!(
                        "{}.{}",
                        i + 1,
                        extension
                    )))
                    .unwrap();

                copied_photos.push(format!("{}.{}", i + 1, extension));
                remove_exif(&photo_target_file);
            }
        }
        None => {
            info!("No photos found for {}, skipping", gpx_file.display());
            return None;
        }
    };

    let mut context = Context::new();
    context.add("track_coordinates", &track_coordinates);
    context.add("article_title", &article_info.title);
    context.add("lon_avg", &article_info.coordinate_avg.lon);
    context.add("lat_avg", &article_info.coordinate_avg.lat);
    context.add("start_time", &article_info.start_time.to_string());
    context.add("end_time", &article_info.end_time.to_string());
    context.add("static_dir", "../static");
    context.add("config", config);
    context.add("copied_photos", &copied_photos);
    context.add("photo_target_dir_relative", &photo_target_dir_relative);

    render_html(
        tera,
        context,
        &target_dir.join("tracks"),
        &article_info.underscored_title,
        "track_article.html",
    )
    .unwrap();

    let track_country = match reverse_geocoding(&article_info.coordinate_avg) {
        Ok(geocoding) => geocoding.address["country"].clone(),
        Err(error) => {
            error!("error while reverse geocoding : {}", error);
            String::new()
        }
    };

    Some(TrackArticle {
        title: article_info.title,
        underscored_title: article_info.underscored_title,
        photos_number: copied_photos.len(),
        country: track_country.to_string(),
        start_time: article_info.start_time,
        end_time: article_info.end_time,
        coordinate_avg: article_info.coordinate_avg,
    })
}

pub fn render_html(
    tera: &Tera,
    context: Context,
    dir: &Path,
    file: &str,
    template: &str,
) -> Result<(), io::Error> {
    let res = tera.render(template, &context).unwrap();

    let mut generated_file = File::create(format!("{}/{}.html", dir.to_str().unwrap(), file))?;

    generated_file.write(res.as_bytes())?;
    Ok(())
}

fn find_photos(
    photos: &Vec<Photo>,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
) -> Option<Vec<&Photo>> {
    let mut res: Vec<&Photo> = Vec::new();

    for p in photos {
        if start_time.timestamp() <= p.datetime.timestamp()
            && end_time.timestamp() >= p.datetime.timestamp()
        {
            res.push(p);
        }
    }

    if res.len() > 0 {
        res.sort_unstable_by_key(|r| r.datetime.timestamp());
        return Some(res);
    }

    None
}

pub fn parse_photos(dir: &Path) -> Vec<Photo> {
    let mut photos: Vec<Photo> = Vec::new();

    unsafe {
        gexiv2_sys::gexiv2_log_set_level(gexiv2_sys::GExiv2LogLevel::MUTE);
    }

    for entry in fs::read_dir(dir).unwrap() {
        let img_path = entry.unwrap().path();
        let file_metadata = rexiv2::Metadata::new_from_path(&img_path.to_str().unwrap()).unwrap();

        if !file_metadata.has_exif() {
            warn!(
                "skipping {}: {}",
                img_path.display(),
                "File doesn't contains Exif metadata"
            );
            continue;
        }

        let datetime_string = file_metadata.get_tag_string("Exif.Image.DateTime").unwrap();

        let datetime_parse =
            match NaiveDateTime::parse_from_str(&datetime_string, "%Y:%m:%d %H:%M:%S") {
                Ok(parse_date) => parse_date,
                Err(error) => {
                    warn!("skipping {}: {}", img_path.display(), error);
                    continue;
                }
            };

        photos.push(Photo {
            path: img_path,
            datetime: datetime_parse,
        });
    }

    photos
}

pub fn generate_index(config: &Config, articles: Vec<TrackArticle>) {
    let target_dir = Path::new(&config.data.site_output);
    let tera = compile_templates!("site/templates/*");
    let mut index_context = Context::new();

    index_context.add("config", &config);
    index_context.add("static_dir", "static");
    index_context.add("articles", &articles);

    render_html(&tera, index_context, &target_dir, "index", "index.html").unwrap();
}

fn remove_exif(img_path: &Path) {
    let file_metadata = rexiv2::Metadata::new_from_path(&img_path.to_str().unwrap()).unwrap();

    if !file_metadata.has_exif() {
        info!(
            "skipping {}: {}",
            img_path.display(),
            "File doesn't contains Exif metadata"
        );
    } else {
        file_metadata.clear();
        file_metadata.save_to_file(&img_path).unwrap();
    }
}

// Get only the country informations (zoom=0) and in French (for now)
// Need error handling
fn reverse_geocoding(coordinate: &Coordinate) -> Result<ReverseGeocoding, Box<error::Error>> {
    let uri = Url::parse_with_params(
        "https://nominatim.openstreetmap.org/reverse.php",
        &[
            ("format", "json"),
            ("lat", &coordinate.lat.to_string()),
            ("lon", &coordinate.lon.to_string()),
            ("accept-language", "fr"),
            ("zoom", "0"),
        ],
    )?;

    let resp: ReverseGeocoding = reqwest::get(uri)?.json().unwrap();
    Ok(resp)
}
