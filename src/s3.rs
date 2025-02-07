// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use s3::creds::Credentials;
use s3::region::Region;
use s3::{Bucket, BucketConfiguration};

use anyhow::Context;
use crate::{Error, Result};

pub async fn connect_bucket(bucket_name: &str, region: &str, endpoint: &str, create: bool) -> Result<Box<Bucket>> {

    let region = Region::Custom {
        region: region.to_owned(),
        endpoint: endpoint.to_owned(),
    };

    let credentials = Credentials::default()
        .with_context(|| "Failed to get AWS credentisals searching in the standard locations.")?;

    let mut bucket =
        Bucket::new(bucket_name, region.clone(), credentials.clone())?
        .with_path_style();

    if !bucket.exists().await? {

        if !create {
            return Err(Error::BucketNotFound(bucket_name.to_owned()).into())
        }

        bucket = Bucket::create_with_path_style(bucket_name, region,
            credentials, BucketConfiguration::default()).await
            .with_context(|| format!("Failed creating bucket {}", bucket_name))?
            .bucket;
    }

    Ok(bucket)
}
