use std::{
    fs::{self},
    path::Path,
};

use clap::Parser;
use reqwest::blocking::multipart;

/// ody-visual command line arguments
#[derive(Parser, Debug)]
#[command(version)]
struct UploaderArgs {
    /// The output folder of data (videos, audio, text logs, etc), no trailing slash
    #[arg(short = 'f', long, env = "ODYSSEUS_DAEMON_OUTPUT_FOLDER")]
    output_folder: String,

    /// The URL of Scylla
    #[arg(short = 'u', long, env = "ODYSSEUS_DAEMON_SCYLLA_URL")]
    scylla_url: String,
}

/// Logger module: upload a data_dump.log file to scylla
fn logger_upload(
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

fn main() {
    let cli = UploaderArgs::parse();

    let client = reqwest::blocking::Client::new();

    let entries = fs::read_dir(Path::new(&cli.output_folder)).expect("Invalid data output folder!");

    println!("Traversing data");
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
                                    && dire.file_name() == "data_dump.log"
                                {
                                    println!("Uploading log file: {:?}", dire.path());
                                    if let Err(err) =
                                        logger_upload(&dire.path(), &cli.scylla_url, &client)
                                    {
                                        eprintln!("Failed to send log to scylla: {}", err);
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

    println!(
        "Done, feel free to clear the inside of the {} directory!",
        cli.output_folder
    );
}
