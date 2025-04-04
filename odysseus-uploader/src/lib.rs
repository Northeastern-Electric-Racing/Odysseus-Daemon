use std::{fs, path::Path};

use reqwest::blocking::multipart;

fn upload_file(
    filepath: &Path,
    timestamp: &str,
    file_name: &str,
    scylla_uri: &str,
    client: &reqwest::blocking::Client,
) -> Result<(), reqwest::Error> {
    let file_name = format!("{}_{}", timestamp, file_name);

    println!("File name: {}", file_name);
    println!("Exists: {}", filepath.exists());
    println!("Is file: {}", filepath.is_file());

    let res = client
        .post(scylla_uri)
        .multipart(
            multipart::Form::new()
                .file(file_name, filepath)
                .expect("Could not fetch file for sending"),
        )
        .send()?;

    res.error_for_status()?;

    Ok(())
}

fn extract_timestamp(input: &str) -> Option<&str> {
    input.split_once('-').map(|(_, timestamp)| timestamp.trim())
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
                            Ok(file) => {
                                if file
                                    .file_type()
                                    .expect("Could not decode filetype")
                                    .is_file()
                                    && upload_logs
                                    && file.file_name() == "data_dump.log"
                                {
                                    println!("Uploading file: {:?}", file.path());
                                    if let Err(err) = upload_file(
                                        &file.path(),
                                        "",
                                        "",
                                        format!("{}/insert/log", scylla_url).as_str(),
                                        &client,
                                    ) {
                                        eprintln!("Failed to send file to scylla: {}", err);
                                    }
                                } else if file
                                    .file_type()
                                    .expect("Could not decode filetype")
                                    .is_file()
                                    && ((upload_video && file.file_name() == "ner24-frontcam.mp4")
                                        || (upload_serial
                                            && (file.file_name() == "cerberus-dump.cap"
                                                || file.file_name() == "shepherd-dump.cap")))
                                {
                                    println!("Inserting file to: {}/insert/file", scylla_url);
                                    if let Some(directory_name) = dire.file_name().to_str() {
                                        if let Some(file_name) = file.file_name().to_str() {
                                            if let Some(timestamp) =
                                                extract_timestamp(directory_name)
                                            {
                                                if let Err(err) = upload_file(
                                                    &file.path(),
                                                    timestamp,
                                                    file_name,
                                                    format!("{}/insert/file", scylla_url).as_str(),
                                                    &client,
                                                ) {
                                                    eprintln!(
                                                        "Failed to send file to scylla: {}",
                                                        err
                                                    );
                                                }
                                            } else {
                                                eprintln!("Could not extract timestamp");
                                            }
                                        } else {
                                            eprintln!("Could not get file name");
                                        };
                                    } else {
                                        eprintln!("Could not get directory name");
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
