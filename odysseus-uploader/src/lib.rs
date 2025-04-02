use std::{fs, path::Path};

use reqwest::blocking::multipart;

fn upload_file(
    filepath: &Path,
    scylla_uri: &str,
    client: &reqwest::blocking::Client,
) -> Result<(), reqwest::Error> {
    let res = client
        .post(format!("{}/insert/file", scylla_uri))
        .multipart(
            multipart::Form::new()
                .file("", filepath)
                .expect("Could not fetch file for sending"),
        )
        .send()?;

    res.error_for_status()?;

    Ok(())
}

pub fn upload_files(
    output_folder: &str,
    scylla_url: &str,
    upload_logs: bool,
    upload_video: bool,
    upload_serial: bool,
) {
    let client = reqwest::blocking::Client::new();

    let entries = fs::read_dir(Path::new(output_folder)).expect("Invalid data output folder!");

    for entry in entries {
        match entry {
            Ok(dire) => {
                if dire
                    .file_type()
                    .expect("Could not decode filetype")
                    .is_dir()
                    && dire
                        .file_name()
                        .into_string()
                        .expect("Could not decode folder names")
                        .starts_with("event-")
                {
                    println!("Entering folder {:?}", dire.file_name());
                    let entries = fs::read_dir(dire.path()).expect("Invalid folder!");
                    for entry in entries {
                        match entry {
                            Ok(dire) => {
                                if dire
                                    .file_type()
                                    .expect("Could not decode filetype")
                                    .is_file()
                                    && ((upload_logs && dire.file_name() == "data_dump.log")
                                        || (upload_video
                                            && dire.file_name() == "ner24-frontcam.mp4")
                                        || (upload_serial
                                            && (dire.file_name() == "cerberus-dump.cap"
                                                || dire.file_name() == "shepherd-dump.cap")))
                                {
                                    println!("Uploading file: {:?}", dire.path());
                                    if let Err(err) = upload_file(&dire.path(), scylla_url, &client)
                                    {
                                        eprintln!("Failed to send file to scylla: {}", err);
                                    }
                                }
                            }
                            Err(e) => eprintln!("Could not traverse folder {}", e),
                        }
                    }
                } else {
                    eprintln!("Invalid item: {:?}", dire);
                }
            }
            Err(e) => eprintln!("Could not traverse folder {}", e),
        }
    }
}
