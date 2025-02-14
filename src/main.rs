// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use clap::Parser;
use s3_cache::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // .env support in aid of Credentials::default()
    dotenvy::dotenv()?;

    let args = Options::parse();

    let bucket = s3_cache::Storage::new(args.bucket.as_str(), args.region.as_str(), args.endpoint.as_str(), false).await?;

    match &args.command {
        Commands::Upload(arg) => {
            println!("Upload {:?}",arg);
            s3_cache::actions::upload(bucket, arg.cache.name.as_str(), &arg.files).await?;
        },
        Commands::Download(arg) => {
            println!("{:?}",arg);
        },
        Commands::Delete(arg) => {
            println!("{:?}",arg);
        },
        Commands::List(arg) => {
            s3_cache::actions::list(bucket, arg.name.as_deref()).await?;
        },
        Commands::Expire(arg) => {
            println!("{:?}",arg);
            s3_cache::actions::expire(bucket).await?;
        },
    }
    Ok(())
}

#[derive(Parser, Debug)]
#[command(author, version, long_about =
"Deduplicating temporary store in S3 for CI artifacts

TBD fill in more details
")]
struct Options {
    #[command(subcommand)]
    command: Commands,

    /// The S3 Bucket
    #[arg(long, global=true, default_value="s3-cache-test")] // TODO default name
    bucket: String,

    /// The S3 endpoint
    #[arg(long, global=true, default_value="http://localhost:9000")]
    endpoint: String,

    /// The S3 region
    #[arg(long, global=true, default_value="global")]
    region: String,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Upload files to cache
    Upload(Upload),
    /// Download files from cache
    Download(Download),
    /// Delete a cache - files will not be accessible, but they won't be deleted.
    Delete(Delete),
    /// List files from a cache
    List(List),
    /// Expire old or unused files from cache
    Expire(Expire),
}

#[derive(clap::Args, Debug)]
struct CacheArgs {
    /// The name of the cache. Required.
    #[arg(long)]
    name: String,
}

#[derive(clap::Args, Debug)]
struct Upload {
    /// Files to cache and upload
    files: Vec<std::path::PathBuf>,

    #[command(flatten)]
    cache: CacheArgs,
}

#[derive(clap::Args, Debug)]
struct Download {
    #[command(flatten)]
    cache: CacheArgs,
}

#[derive(clap::Args, Debug)]
struct List {
    /// The name of the cache to list. If not presented list the caches.
    #[arg(long)]
    name: Option<String>,
}

#[derive(clap::Args, Debug)]
struct Delete {
    #[command(flatten)]
    cache: CacheArgs,
}

#[derive(clap::Args, Debug)]
struct Expire {
}

// Claps' built-in self test
#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Options::command().debug_assert()
}
