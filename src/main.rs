// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use clap::Parser;
use s3_cache::Result;
use std::path::PathBuf;
use::std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    // .env support in aid of Credentials::default()
    let dotenv = dotenvy::dotenv();

    let args = Options::parse();

    let mut logger = env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or(
                if args.debug { "debug" }
                else if args.verbose { "info" }
                else { "warn" }
            ));
    if !args.debug {
        logger.format(|buf, record| {
            writeln!(buf, "{}", record.args())
        });
    }
    logger.format_timestamp(None).init();

    if let Ok(path) = dotenv {
        log::info!("Loaded environment from {:?}", path);
    }
    log::debug!("args={:?}", args);

    let bucket = s3_cache::Storage::new_dangerous(args.bucket.as_str(), args.region.as_str(), args.endpoint.as_str(), false, args.skip_cert_validation).await
        .inspect_err(|_| {
            println!("\nFailed to initialise connection to S3.\n\nCheck AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY environment\nvariables are set.\n");
        })?;

    match &args.command {
        Commands::Upload(arg) => {
            s3_cache::actions::upload(bucket, arg.cache.name.as_str(), &arg.files, arg.threshold).await?;
        },
        Commands::Download(arg) => {
            s3_cache::actions::download(bucket, arg.cache.name.as_str(), arg.outpath.clone()).await?;
        },
        Commands::Delete(arg) => {
            s3_cache::actions::delete(bucket, arg.cache.name.as_str()).await?;
        },
        Commands::List(arg) => {
            s3_cache::actions::list(bucket, arg.name.as_deref()).await?;
        },
        Commands::Expire(arg) => {
            s3_cache::actions::expire(bucket, arg.days).await?;
        },
    }
    Ok(())
}

#[derive(Parser, Debug)]
#[command(author, version, long_about =
"Deduplicating temporary store in S3 for CI artifacts

AWS credentials are loaded from environment variables (or .env
file). eg:

   AWS_ACCESS_KEY_ID=8dq14eEakqwmEko9XjUd
   AWS_SECRET_ACCESS_KEY=0TX3ZyiadJIC7w7NPqbeu7VzKcbHDheVovq7UB9rOBw
")]
struct Options {
    #[command(subcommand)]
    command: Commands,

    /// The S3 Bucket
    #[arg(long, global=true, default_value="s3-cache-test", env="S3_CACHE_BUCKET")] // TODO default name
    bucket: String,

    /// The S3 endpoint
    #[arg(long, global=true, default_value="http://localhost:9000", env="S3_CACHE_ENDPOINT")]
    endpoint: String,

    /// The S3 region
    #[arg(long, global=true, default_value="global", env="S3_CACHE_REGION")]
    region: String,

    /// Skip HTTPS certificate validation.  This affects security.  Use with care.
    #[arg(long, global=true, env="S3_CACHE_SKIP_CERT_VALIDATION")]
    skip_cert_validation: bool,

    /// Add additional debug output
    #[arg(long, global=true)]
    debug: bool,

    /// Add additional output
    #[arg(long, global=true)]
    verbose: bool,
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

    /// Expire old or unused files from cache.  Currently only age is implemented.
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
    files: Vec<PathBuf>,

    #[command(flatten)]
    cache: CacheArgs,

    /// Dedupe file threshold size in bytes: files below this size
    /// will just be stored with the cache and not deduplicated
    #[arg(long, default_value_t=25*1024*1024)]
    threshold: usize,
}

#[derive(clap::Args, Debug)]
struct Download {
    #[command(flatten)]
    cache: CacheArgs,

    /// Where to put the output
    #[arg(long, short='o', default_value=".")]
    outpath: PathBuf
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

    /// Age of objects to expire unconditionally
    #[arg(long, default_value_t=14)]
    days: u32,
}

// Claps' built-in self test
#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Options::command().debug_assert()
}
