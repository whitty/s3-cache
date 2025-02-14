// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use std::path::{Path, PathBuf};

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

    pub async fn put_file_unless_exists<R: tokio::io::AsyncRead + Unpin + ?Sized>(
        &self, reader: &mut R, s3_path: &str) -> Result<()> {

        let connection = self.connect().await?;

        if connection.exists(s3_path).await? {
            println!("File {} exists", s3_path);
            return Ok(());
        }

        connection.put_file(reader, s3_path).await
    }

    pub async fn list_dirs(&self, path: &str) -> Result<Vec<String>> {
        // Async variant with `tokio` or `async-std` features
        let connection = self.connect().await?;

        connection.list_dirs(path).await
    }

    pub async fn put_file<R: tokio::io::AsyncRead + Unpin + ?Sized>(
        &self, reader: &mut R, s3_path: &str) -> Result<()> {

        let connection = self.connect().await?;

        connection.put_file(reader, s3_path).await
    }

    pub async fn get_file<W: tokio::io::AsyncWrite + Send + Unpin + ?Sized>(
        &self, writer: &mut W, s3_path: &str) -> Result<()> {

        let connection = self.connect().await?;

        connection.get_file_stream(s3_path, writer).await
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

    async fn exists(&self, path: &str) -> Result<bool> {
        let result = self.head(path).await;
        match result {
            Ok(_r) => {
                // println!("last_modified={}", _r.last_modified.unwrap_or("".into()));
                // println!("content_length={}", _r.content_length.unwrap_or(0));
                Ok(true)
            },
            Err(Error::S3Error(s3::error::S3Error::HttpFailWithBody(404 ,_))) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn put_file<R: tokio::io::AsyncRead + Unpin + ?Sized>(
        &self, reader: &mut R, s3_path: &str) -> Result<()> {
        let _response = self.bucket.put_object_stream(reader, s3_path).await?;

        // println!("put({}) response={:?} {}", s3_path, _response, _response.status_code());
        // assert_eq!(_response.status_code(), 200);
        Ok(())
    }

    async fn get_file_stream<W: tokio::io::AsyncWrite + Send + Unpin + ?Sized>(&self, s3_path: &str, w: &mut W) -> Result<()> {
        let _code = self.bucket.get_object_to_writer(s3_path, w).await?;
        // println!("code={}", _code);
        Ok(())
    }

    async fn delete(&self, s3_path: &str) -> Result<()> {
        let _response = self.bucket.delete_object(s3_path).await?;

        println!("deleted '{}'", s3_path);
        // println!("delete({}) response={:?} {}", s3_path, response, response.status_code());
        // assert_eq!(_response.status_code(), 204);
        Ok(())
    }

    async fn head(&self, path: &str) -> Result<s3::serde_types::HeadObjectResult> {
        let (head_object_result, _code) = self.bucket.head_object(path).await?;

        // println!("code={}", _code);
        // println!("head_object_result={:?}", head_object_result);
        Ok(head_object_result)
    }

    // What a fuss the error handling stuff is a mess to put together, so split into pieces
    fn strip_(p: PathBuf, prefix: &std::path::Path) -> Result<PathBuf> {
        let cp_prefix = p.clone();
        cp_prefix.strip_prefix(prefix).map_err(|_| Error::InvalidPath(p)).map(|x| x.into())
    }

    fn strip(p: PathBuf, prefix: &std::path::Path) -> Result<String> {
        Self::path_to_str(Connection::strip_(p, prefix)?.as_ref())
    }

    fn path_to_str(p: &Path) -> Result<String> {
        p.to_str().map(String::from).ok_or_else(|| Error::InvalidPath(PathBuf::from(p)))
    }

    async fn list_dirs(&self, path: &str) -> Result<Vec<String>> {
        let prefix = PathBuf::from(path);
        for result in self.bucket.list(String::from(path), Some("/".to_string())).await? {

            if let Some(prefs) = result.common_prefixes {
                return prefs.into_iter().map(|cp| {
                    Connection::strip(PathBuf::from(cp.prefix), &prefix)
                }).collect();
            }
        }
        Ok(vec![])
    }

}
