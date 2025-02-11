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

        match s.connect().await {
            Ok(_) => Ok(s),
            Err(Error::BucketNotFound(x)) => {

                if !create {
                    return Err(Error::BucketNotFound(x))
                }
                s.create().await?;
                Ok(s)
            },
            Err(e) => Err(e),
        }
    }

    async fn connect(&self) -> Result<Connection> {
        let bucket = Bucket::new(self.bucket_name.as_str(), self.region.clone(), self.credentials.clone())?
            .with_path_style();
        if !bucket.exists().await? {
            return Err(Error::BucketNotFound(self.bucket_name.to_owned()))
        }
        Ok(Connection { bucket })
    }

    async fn create(&self) -> Result<Connection> {
        let bucket = Bucket::create_with_path_style(
            self.bucket_name.as_str(), self.region.clone(),
            self.credentials.clone(), BucketConfiguration::default()).await
            .map_err(Error::BucketCreationError)?
            .bucket;
        Ok(Connection { bucket })
    }

    pub async fn put_file<R: tokio::io::AsyncRead + Unpin + ?Sized>(
        &self, reader: &mut R, s3_path: &str) -> Result<()> {

        let connection = self.connect().await?;

        if connection.exists(s3_path).await? {
            println!("File {} exists", s3_path);
            return Ok(());
        }

        connection.put_file(reader, s3_path).await
    }

    pub async fn delete(&self, s3_path: &str) -> Result<()> {

        let connection = self.connect().await?;

        connection.delete(s3_path).await
    }
}

struct Connection {
    bucket: Box<Bucket>,
}

impl Connection {

    pub async fn exists(&self, path: &str) -> Result<bool> {
        let result = self.head(path).await;
        match result {
            Ok(r) => {
                println!("last_modified={}", r.last_modified.unwrap_or("".into()));
                println!("content_length={}", r.content_length.unwrap_or(0));
                Ok(true)
            },
            Err(Error::S3Error(s3::error::S3Error::HttpFailWithBody(404 ,_))) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn put_file<R: tokio::io::AsyncRead + Unpin + ?Sized>(
        &self, reader: &mut R, s3_path: &str) -> Result<()> {
        let response = self.bucket.put_object_stream(reader, s3_path).await?;

        println!("put({}) response={:?} {}", s3_path, response, response.status_code());
        assert_eq!(response.status_code(), 200);
        Ok(())
    }

    pub async fn delete(&self, s3_path: &str) -> Result<()> {
        let response = self.bucket.delete_object(s3_path).await?;

        println!("delete({}) response={:?} {}", s3_path, response, response.status_code());
        assert_eq!(response.status_code(), 204);
        Ok(())
    }

    async fn head(&self, path: &str) -> Result<s3::serde_types::HeadObjectResult> {
        let (head_object_result, code) = self.bucket.head_object(path).await?;

        println!("code={}", code);
        println!("head_object_result={:?}", head_object_result);
        Ok(head_object_result)
    }
}
