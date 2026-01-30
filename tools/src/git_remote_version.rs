// This code is noted as an option we could pull into the tool set, but the problem with it is
// that its dependence on `reqwest` to interface with github to try and guess semantic versions
// pulls a huge amount of code into the dependency tree that I prefer not to have in the SBOM.
// Thus to get a semantic version users have to either check out tags, assign a version manually,
// or just go with a nil-rev on the versioning.

use std::collections::HashMap;
use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CargoToml {
    dependencies: Option<HashMap<String, Dependency>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum Dependency {
    Simple(String),
    Detailed(DetailedDependency),
}

#[derive(Debug, Deserialize, Clone)]
struct DetailedDependency {
    git: Option<String>,
    rev: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubTag {
    name: String,
}

fn find_xous_core_revision(cargo_toml_path: &str) -> Result<Option<String>> {
    // Read and parse Cargo.toml
    let contents = fs::read_to_string(cargo_toml_path).context("Failed to read Cargo.toml")?;

    let cargo: CargoToml = toml::from_str(&contents).context("Failed to parse Cargo.toml")?;

    let Some(deps) = cargo.dependencies else {
        return Ok(None);
    };

    let xous_core_url = "https://github.com/betrusted-io/xous-core";
    let mut found_revisions = Vec::new();

    // Search through dependencies for xous-core references
    for (name, dep) in deps.iter() {
        if let Dependency::Detailed(detailed) = dep {
            if let (Some(git), Some(rev)) = (&detailed.git, &detailed.rev) {
                if git == xous_core_url || git == &format!("{}.git", xous_core_url) {
                    found_revisions.push((name.clone(), rev.clone()));
                }
            }
        } else if let Dependency::Simple(simple) = dep {
            unimplemented!("Can't handle dep: {}", simple);
        }
    }

    if found_revisions.is_empty() {
        return Ok(None);
    }

    if found_revisions.len() > 1 {
        eprintln!("Warning: Found multiple xous-core revisions:");
        for (name, rev) in &found_revisions {
            eprintln!("  {} -> {}", name, rev);
        }
        eprintln!("Using the first one: {}", found_revisions[0].1);
    }

    Ok(Some(found_revisions[0].1.clone()))
}

fn get_latest_tag_from_github(repo: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/tags", repo);

    let client = reqwest::blocking::Client::builder()
        .user_agent("xous-app-uf2-tool") // GitHub requires a user agent
        .build()?;

    let mut request = client.get(&url);

    // Optional: use token if available (no token required by default)
    let using_token = if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.header("Authorization", format!("token {}", token));
        true
    } else {
        false
    };

    let response = request.send().context("Failed to fetch tags from GitHub")?;

    // Check for rate limit errors
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        // Check if this is a rate limit error
        if let Some(remaining) = response.headers().get("x-ratelimit-remaining") {
            if remaining.to_str().unwrap_or("") == "0" {
                if using_token {
                    anyhow::bail!(
                        "GitHub API rate limit exceeded (5000 requests/hour with token).\n\
                         Please wait before trying again."
                    );
                } else {
                    anyhow::bail!(
                        "GitHub API rate limit exceeded (60 requests/hour per IP without authentication).\n\
                         \n\
                         To increase your rate limit to 5000 requests/hour, set the GITHUB_TOKEN environment variable:\n\
                         \n\
                         1. Create a GitHub Personal Access Token at:\n\
                            https://github.com/settings/tokens\n\
                         2. No special permissions needed - you can leave all scopes unchecked\n\
                         3. Set the token:\n\
                            Linux/Mac:   export GITHUB_TOKEN=your_token_here\n\
                            Windows:     set GITHUB_TOKEN=your_token_here\n\
                         \n\
                         For more info: https://docs.github.com/en/rest/overview/rate-limits-for-the-rest-api"
                    );
                }
            }
        }
    }

    if !response.status().is_success() {
        anyhow::bail!("GitHub API request failed: {}", response.status());
    }

    let tags: Vec<GitHubTag> = response.json().context("Failed to parse GitHub API response")?;

    tags.first().map(|tag| tag.name.clone()).ok_or_else(|| anyhow::anyhow!("No tags found in repository"))
}

pub fn get_xous_version() -> Result<String> {
    // Assume we're running in the root directory
    let cargo_toml_path = "Cargo.toml";

    println!("Parsing {} for xous-core dependencies...", cargo_toml_path);

    let rev = find_xous_core_revision(cargo_toml_path)?
        .ok_or_else(|| anyhow::anyhow!("No xous-core git dependency found in Cargo.toml"))?;

    println!("Found xous-core revision: {}", rev);
    println!("Fetching latest tag from GitHub...");

    let version = get_latest_tag_from_github("betrusted-io/xous-core")?;

    println!("Latest xous-core version: {}", version);

    Ok(version)
}
