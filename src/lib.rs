use chrono::{DateTime, NaiveDateTime, Utc};
use exif::{Reader, Tag};
use gpx::read;
use gpx::TrackSegment;
use gpx::{Gpx, Track};
use log::*;
use serde_derive::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use tera::{Context, Tera};

#[derive(Serialize)]
struct Coordinate {
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

pub fn gpx_to_html(gpx_file: &Path, target_dir: &Path, tera: &Tera, config: &Config) {
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

    let start_time = segment.points.first().unwrap().time.unwrap();
    let end_time = segment.points.last().unwrap().time.unwrap();

    let gpx_html_title = match gpx.metadata.unwrap().name {
        Some(name) => name,
        None => gpx_file.file_stem().unwrap().to_str().unwrap().to_string(),
    };

    let gpx_name = gpx_html_title.replace(" ", "_");

    let img_input_dir = Path::new(&config.data.img_input);
    let dates = parse_photos(img_input_dir);
    let photos = find_photos(dates, start_time, end_time);
    let mut copied_photos: Vec<String> = Vec::new();
    let photo_target_dir = target_dir.join("static/photos").join(gpx_name.to_string());
    let photo_target_dir_relative = Path::new("static/photos").join(gpx_name.to_string());

    match photos {
        Some(photos) => {
            let photos = photos;

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
            }
        }
        None => {
            info!("No photos found for {}", gpx_file.display());
        }
    };

    let mut context = Context::new();
    context.add("track_coordinates", &track_coordinates);
    context.add("gpx_html_title", &gpx_html_title);
    context.add("lon_avg", &lon_avg);
    context.add("lat_avg", &lat_avg);
    context.add("start_time", &start_time.to_string());
    context.add("end_time", &end_time.to_string());
    context.add("static_dir", "../static");
    context.add("config", config);
    context.add("copied_photos", &copied_photos);
    context.add("photo_target_dir_relative", &photo_target_dir_relative);

    render_html(tera, context, &target_dir.join("tracks"), &gpx_name).unwrap();
}

fn render_html(tera: &Tera, context: Context, dir: &Path, file: &str) -> Result<(), io::Error> {
    let res = tera.render("track.html", &context).unwrap();

    let mut generated_file = File::create(format!("{}/{}.html", dir.to_str().unwrap(), file))?;

    generated_file.write(res.as_bytes())?;
    Ok(())
}

fn find_photos(
    photos: Vec<Photo>,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
) -> Option<Vec<Photo>> {
    let mut res: Vec<Photo> = Vec::new();

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

    for entry in fs::read_dir(dir).unwrap() {
        let img_path = entry.unwrap().path();
        let file = File::open(&img_path).unwrap();
        let reader = match Reader::new(&mut std::io::BufReader::new(&file)) {
            Ok(exif) => exif,
            Err(error) => {
                warn!("skipping {}: {}", img_path.display(), error);
                continue;
            }
        };

        let datetime_value = &reader.get_field(Tag::DateTime, false).unwrap().value;
        let datetime_string = datetime_value.display_as(Tag::DateTime).to_string();
        let datetime_parse =
            match NaiveDateTime::parse_from_str(&datetime_string, "%Y-%m-%d %H:%M:%S") {
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
