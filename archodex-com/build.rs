use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use http_body_util::BodyExt as _;

const GITHUB_REPO_OWNER: &str = "Archodex";
const GITHUB_REPO_NAME: &str = "archodex-backend-archodex-com";

#[tokio::main]
async fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");

    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let cache_dir = PathBuf::from(&out_dir).join("archodex-com-cache");

    // Create cache directory if it doesn't exist
    fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

    // Check latest version from GitHub
    let version_string = env!("CARGO_PKG_VERSION");
    let latest_sha = version_string
        .split('+')
        .nth(1)
        .unwrap_or_else(|| panic!("CARGO_PKG_VERSION does not contain git sha: {version_string}"));

    let archive_path = cache_dir.join(format!("{latest_sha}.tar.gz"));

    // Check if we already have this archive
    if !archive_path.exists() {
        // Get GitHub client with authentication
        let octocrab = match create_authenticated_client().await {
            Ok(client) => client,
            Err(e) => {
                println!("cargo:warning=Failed to authenticate with GitHub: {e}");
                println!(
                    "cargo:warning=Please set GITHUB_TOKEN or GH_TOKEN environment variable, or configure git credentials"
                );
                panic!("Authentication required to access private repository");
            }
        };

        // Download the archive
        if let Err(e) = download_archive(&octocrab, latest_sha, &archive_path).await {
            panic!("cargo:warning=Failed to download archive: {e}");
        }
    }

    // Extract the archive
    if let Err(e) = extract_archive(&archive_path, &manifest_dir) {
        panic!("Failed to extract archive at {archive_path:?}: {e}");
    }

    // Clean up old archives
    for entry in fs::read_dir(&cache_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path != archive_path {
            let _ = fs::remove_file(path);
        }
    }
}

async fn create_authenticated_client() -> Result<octocrab::Octocrab, Box<dyn std::error::Error>> {
    // First try standard environment variables
    if let Ok(token) = env::var("GITHUB_TOKEN").or_else(|_| env::var("GH_TOKEN")) {
        return Ok(octocrab::Octocrab::builder()
            .personal_token(token)
            .build()?);
    }

    // Try git credential helper
    if let Ok(token) = get_git_credential_token() {
        return Ok(octocrab::Octocrab::builder()
            .personal_token(token)
            .build()?);
    }

    Err(
        "No GitHub authentication found. Please set GITHUB_TOKEN or configure git credentials."
            .into(),
    )
}

fn get_git_credential_token() -> Result<String, Box<dyn std::error::Error>> {
    let mut child = Command::new("git")
        .args(["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    // Write the request to git credential
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "protocol=https")?;
        writeln!(stdin, "host=github.com")?;
        writeln!(stdin)?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err("git credential helper failed".into());
    }

    // Parse the output to find the password/token
    let output_str = String::from_utf8(output.stdout)?;
    for line in output_str.lines() {
        if let Some(token) = line.strip_prefix("password=") {
            return Ok(token.to_string());
        }
    }

    Err("No token found in git credential output".into())
}

async fn download_archive(
    octocrab: &octocrab::Octocrab,
    sha: &str,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_handler = octocrab.repos(GITHUB_REPO_OWNER, GITHUB_REPO_NAME);

    let response = repo_handler.download_tarball(sha.to_string()).await?;

    if !response.status().is_success() {
        return Err(format!("Failed to download archive: {}", response.status()).into());
    }

    // Download the content
    let bytes = response.into_body().collect().await?.to_bytes().to_vec();
    fs::write(output_path, bytes)?;

    Ok(())
}

fn extract_archive(
    archive_path: &Path,
    manifest_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // GitHub adds a prefix to the archive, we need to find it
    let mut root_prefix = String::new();

    // First pass: find the root prefix
    let tar_gz = fs::File::open(archive_path)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut temp_archive = tar::Archive::new(tar);

    for entry in temp_archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        let path_str = path.to_string_lossy();

        if let Some(pos) = path_str.find('/') {
            root_prefix = path_str[..=pos].to_string();
            break;
        }
    }

    let tar_gz = fs::File::open(archive_path)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);

    // Second pass: extract files
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let path_str = path.to_string_lossy();

        // Strip the root prefix if present
        let relative_path = if path_str.starts_with(&root_prefix) {
            &path_str[root_prefix.len()..]
        } else {
            &path_str
        };

        // Skip empty paths
        if relative_path.is_empty() {
            continue;
        }

        let dest_path = manifest_dir.join(relative_path);

        // Create parent directories if needed
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Extract the file only if it's different
        if entry.header().entry_type().is_file() {
            // Read the new content into memory
            let mut new_content = Vec::new();
            std::io::copy(&mut entry, &mut new_content)?;

            // Check if we need to write the file
            let should_write = if dest_path.exists() {
                // Compare with existing content
                match fs::read(&dest_path) {
                    Ok(existing_content) => {
                        if existing_content != new_content {
                            println!(
                                "cargo:warning=File {relative_path:?} is different than mainline"
                            );
                            false
                        } else {
                            false
                        }
                    }
                    Err(_) => true, // If we can't read the existing file, write the new one
                }
            } else {
                true // File doesn't exist, so write it
            };

            if should_write {
                fs::write(&dest_path, new_content)?;
                println!("cargo:warning=Restored file {relative_path:?}");
            }
        }
    }

    Ok(())
}
