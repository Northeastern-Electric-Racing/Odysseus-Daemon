use std::{fs, path::Path, time::Duration};

use chrono::{DateTime, TimeZone, Utc};
use reqwest::{Client, multipart};

async fn upload_file(
    filepath: &Path,
    timestamp: String,
    file_name: &str,
    scylla_uri: &str,
    client: &reqwest::Client,
) -> Result<(), reqwest::Error> {
    let file_name = format!("{timestamp}_{file_name}");

    println!("Sending file: {file_name}");

    let res = client
        .post(scylla_uri)
        .multipart(
            multipart::Form::new()
                .file(file_name, filepath)
                .await
                .expect("Failed to create multipart form"),
        )
        .timeout(Duration::from_secs(300)) // Five minute timeout to ensure that at least all the requests will eventually finish. Files shouldnt required this long to send ideally when using 2.4 Hermes
        .send()
        .await?;

    res.error_for_status()?;

    Ok(())
}

fn extract_timestamp(input: &str) -> Option<String> {
    // Split on the first '-' and parse the timestamp
    let raw_ts = input.split_once('-')?.1.trim();

    // Parse as milliseconds since epoch
    let millis: i64 = raw_ts.parse().ok()?;
    let datetime: DateTime<Utc> = Utc.timestamp_millis_opt(millis).single()?;

    // Format to MM/DD/YYYY-HH::mm::ss
    Some(datetime.format("%m-%d-%Y_%H_%M_%S").to_string())
}

pub fn upload_files(
    output_folder: &str,
    scylla_url: &str,
    upload_logs: bool,
    upload_video: bool,
    upload_serial: bool,
) -> tokio::task::JoinHandle<()> {
    let output_folder = output_folder.to_string();
    let scylla_url = scylla_url.to_string();

    tokio::spawn(async move {
        let client = Client::new();

        let entries = fs::read_dir(Path::new(&output_folder)).expect("Invalid data output folder!");

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
                                    let is_file = file
                                        .file_type()
                                        .expect("Could not decode filetype")
                                        .is_file();

                                    let file_name = file.file_name();
                                    let path = file.path();

                                    if is_file && upload_logs && file_name == "data_dump.log" {
                                        println!("Uploading file: {path:?}");
                                        let client = client.clone();
                                        let scylla_url = scylla_url.clone();
                                        if let Err(err) = upload_file(
                                            &path,
                                            "".to_string(),
                                            "",
                                            format!("{scylla_url}/insert/log").as_str(),
                                            &client,
                                        )
                                        .await
                                        {
                                            eprintln!("Failed to send file to scylla: {err}");
                                        }
                                    } else if is_file
                                        && ((upload_video && file_name == "ner24-frontcam.mp4")
                                            || (upload_serial
                                                && (file_name == "cerberus-dump.cap"
                                                    || file_name == "shepherd-dump.cap")))
                                    {
                                        let client = client.clone();
                                        let scylla_url = scylla_url.clone();
                                        let directory_name = dire.file_name();
                                        if let Some(directory_name) = directory_name.to_str() {
                                            if let Some(file_name) = file_name.to_str() {
                                                if let Some(timestamp) =
                                                    extract_timestamp(directory_name)
                                                {
                                                    if let Err(err) = upload_file(
                                                        &path,
                                                        timestamp,
                                                        file_name,
                                                        format!("{scylla_url}/insert/file")
                                                            .as_str(),
                                                        &client,
                                                    )
                                                    .await
                                                    {
                                                        eprintln!(
                                                            "Failed to send file to scylla: {err}"
                                                        );
                                                    }
                                                } else {
                                                    eprintln!("Could not extract timestamp");
                                                }
                                            } else {
                                                eprintln!("Could not get file name");
                                            }
                                        } else {
                                            eprintln!("Could not get directory name");
                                        }
                                    }
                                }
                                Err(e) => eprintln!("Could not traverse folder {e}"),
                            }
                        }
                    } else {
                        eprintln!("Invalid item: {dire:?}");
                    }
                }
                Err(e) => eprintln!("Could not traverse folder {e}"),
            }
        }
    })

    // Function returns immediately
}
