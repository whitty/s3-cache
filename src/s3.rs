// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use s3::creds::Credentials;
use s3::region::Region;
use s3::{Bucket, BucketConfiguration};

use crate::Error;

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Storage {
    bucket_name: String,
    region: Region,
    credentials: Credentials,
}

impl Storage {

    pub async fn new(bucket_name: &str, region: &str, endpoint: &str, create: bool) -> Result<Storage> {

        let region = Region::Custom {
            region: region.to_owned(),
            endpoint: endpoint.to_owned(),
        };

        let credentials = Credentials::default()?;

        let s = Storage {
            bucket_name: bucket_name.to_owned(),
            region, credentials,
        };

        let bucket = s.connect_bucket().await?;

        if !bucket.exists().await? {
            if !create {
                return Err(Error::BucketNotFound(bucket_name.to_owned()))
            }
            s.create_bucket().await?;
        }

        Ok(s)
    }

    async fn connect_bucket(&self) -> Result<Box<Bucket>> {
        let bucket = Bucket::new(self.bucket_name.as_str(), self.region.clone(), self.credentials.clone())?
            .with_path_style();
        Ok(bucket)
    }

    async fn create_bucket(&self) -> Result<Box<Bucket>> {
        let bucket = Bucket::create_with_path_style(
            self.bucket_name.as_str(), self.region.clone(),
            self.credentials.clone(), BucketConfiguration::default()).await
            .map_err(Error::BucketCreationError)?
            .bucket;
        Ok(bucket)
    }

    pub async fn exists(&self, path: &str) -> Result<bool> {
        let result = self.head(path).await;
        match result {
            Ok(_) => {
                Ok(true)
            },
            Err(Error::S3Error(s3::error::S3Error::HttpFailWithBody(404 ,_))) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn head(&self, path: &str) -> Result<s3::serde_types::HeadObjectResult> {
        let bucket = self.connect_bucket().await?;

        let (head_object_result, _) = bucket.head_object(path).await?;
        Ok(head_object_result)
    }
}
