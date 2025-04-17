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
    accept_invalid_certs: bool,
}

impl Storage {

    // TODO replace this with a builder
    pub async fn new(bucket_name: &str, region: &str, endpoint: &str, create: bool) -> Result<Storage> {
        Self::new_dangerous(bucket_name, region, endpoint, create, false).await
    }

    pub async fn new_dangerous(bucket_name: &str, region: &str, endpoint: &str, create: bool, accept_invalid_certs: bool) -> Result<Storage> {

        let region = Region::Custom {
            region: region.to_owned(),
            endpoint: endpoint.to_owned(),
        };

        let credentials = Credentials::default()?;

        let s = Storage {
            bucket_name: bucket_name.to_owned(),
            region, credentials,
            accept_invalid_certs,
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
            .set_dangereous_config(self.accept_invalid_certs, false)?
            .with_path_style();

        let connection = Connection { bucket };
        connection.check_connect().await?;
        Ok(connection)
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
            log::info!("File {} exists, not putting", s3_path);
            return Ok(());
        }

        connection.put_file(reader, s3_path).await
    }

    pub async fn list_dirs(&self, path: &str) -> Result<Vec<String>> {
        // Async variant with `tokio` or `async-std` features
        let connection = self.connect().await?;

        connection.list_dirs(path).await
    }

    pub async fn recursive_delete_p(&self, path: &Path) -> Result<()> {
        self.recursive_delete(Connection::path_to_str(path)?.as_ref()).await
    }

    pub async fn recursive_delete(&self, path: &str) -> Result<()> {
        // Async variant with `tokio` or `async-std` features
        let connection = self.connect().await?;

        connection.recursive_delete(path).await
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

    pub async fn recursive_expire(&self, path: impl AsRef<str>,
                                  expiry_time: chrono::DateTime<chrono::Utc>) -> Result<()> {
        let connection = self.connect().await?;
        connection.recursive_expire(path, expiry_time).await
    }
}

struct Connection {
    bucket: Box<Bucket>,
}

impl Connection {

    async fn check_connect(&self) -> Result<bool> {
        // Doesn't work set_dangereous_config - so fake it with list_dirs
        // if !self.bucket.exists().await? {
        //     return Err(Error::BucketNotFound(self.bucket.name.to_owned()))
        // }

        let result = self.bucket.list(String::from(""), Some(String::from("/"))).await;
        if let Err(s3::error::S3Error::HttpFailWithBody(404, message)) = result {
            if message.contains("NoSuchBucket") {
                return Err(Error::BucketNotFound(self.bucket.name.to_owned()));
            } else {
                return Err(s3::error::S3Error::HttpFailWithBody(404, message).into());
            }
        }
        result?;
        Ok(true)
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let result = self.head(path).await;
        match result {
            Ok(_r) => {
                log::debug!("exists: {} last_modified={} content_length={}", path,
                            _r.last_modified.unwrap_or("".into()),
                            _r.content_length.unwrap_or(0));
                Ok(true)
            },
            Err(Error::S3Error(s3::error::S3Error::HttpFailWithBody(404 ,_))) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn validate_path(s: &str) {
        assert!(s.find('\\').is_none(), "invalid path {:?}", s);
    }

    async fn put_file<R: tokio::io::AsyncRead + Unpin + ?Sized>(
        &self, reader: &mut R, s3_path: impl AsRef<str>) -> Result<()> {
        Self::validate_path(s3_path.as_ref());
        let response = self.bucket.put_object_stream(reader, s3_path.as_ref()).await?;

        if response.status_code() != 200 {
            log::warn!("put_file: unexpected response {} putting {}", response.status_code(), s3_path.as_ref());
        }
        Ok(())
    }

    async fn get_file_stream<W: tokio::io::AsyncWrite + Send + Unpin + ?Sized>(&self, s3_path: impl AsRef<str>, w: &mut W) -> Result<()> {
        Self::validate_path(s3_path.as_ref());
        let code = self.bucket.get_object_to_writer(s3_path.as_ref(), w).await?;

        if code != 200 {
            log::warn!("get_file_stream: unexpected response {} getting {}", code, s3_path.as_ref());
        }
        Ok(())
    }

    async fn delete(&self, s3_path: impl AsRef<str>) -> Result<()> {
        Self::validate_path(s3_path.as_ref());
        let response = self.bucket.delete_object(s3_path.as_ref()).await?;

        log::info!("deleted '{}'", s3_path.as_ref());

        if response.status_code() != 204 {
            log::warn!("delete: unexpected response {} deleting {}", response.status_code(), s3_path.as_ref());
        }
        Ok(())
    }

    async fn head(&self, path: impl AsRef<str>) -> Result<s3::serde_types::HeadObjectResult> {
        Self::validate_path(path.as_ref());
        let (head_object_result, _code) = self.bucket.head_object(path).await?;
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

    async fn list_dirs(&self, path: impl AsRef<str>) -> Result<Vec<String>> {
        Self::validate_path(path.as_ref());
        let prefix = PathBuf::from(path.as_ref());
        for result in self.bucket.list(String::from(path.as_ref()), Some("/".to_string())).await? {

            if let Some(prefs) = result.common_prefixes {
                return prefs.into_iter().map(|cp| {
                    Connection::strip(PathBuf::from(cp.prefix), &prefix)
                }).collect();
            }
        }
        Ok(vec![])
    }

    async fn recursive_visit_<F, Fut>(&self, path: impl AsRef<str>, f: F) -> Result<()>
     where F: Sync + Send + Fn(String) -> Fut,
           Fut: std::future::Future<Output = Result<()>>
    {
        let mut work = Vec::<String>::new();
        work.push(String::from(path.as_ref()));

        while let Some(path) = work.pop() {
            Self::validate_path(path.as_ref());

            for result in self.bucket.list(path, Some("/".to_string())).await? {

                for file in result.contents {
                    f(file.key.to_owned()).await?;
                }

                if let Some(prefs) = result.common_prefixes {
                    for pref in prefs {
                        work.push(pref.prefix);
                    }
                }
            }
        }

        Ok(())
    }

    async fn recursive_delete(&self, path: impl AsRef<str>) -> Result<()> {
        self.recursive_visit_(path, |x| async {
            let p = x.clone();
            if let Err(e) = self.delete(x).await {
                log::warn!("Error deleting '{:?}': {}, continuing...", p, e);
            }
            Ok(()) // squash the error and continue
        }).await
    }

    async fn recursive_expire(&self, path: impl AsRef<str>,
                              expiry_time: chrono::DateTime<chrono::Utc>) -> Result<()> {
        log::debug!("recursive_expire {} older than {}", path.as_ref(), &expiry_time);
        self.recursive_visit_(path, |obj_path| async {
            let p = obj_path.clone();

            match self.head(obj_path).await {
                Ok(result) => {
                    match result.last_modified.ok_or(Error::OptionWasNoneError)
                        .and_then(|d| chrono::DateTime::parse_from_rfc2822(d.as_ref())
                                  .map_err(Error::DateTimeParseError)) {
                            Ok(modified) => {
                                if modified < expiry_time {
                                    if let Err(e) =  self.delete(&p).await {
                                        log::info!("Failed to delete expired object '{:?}': {}: continuing...", &p, e);
                                    }
                                }
                            },
                            Err(e) => {
                                log::info!("Unable to find modification time while expiring '{:?}': {}: continuing...", &p, e);
                                if let Err(e) = self.delete(&p).await {
                                    log::debug!("Delete failed on object '{:?}' that doesn't have valid modification time: {}", p, e);
                                }
                            }
                    }
                },
                Err(e) => {
                    // if its not there - try deleting it
                    log::warn!("Error calling head while expiring '{:?}': {}: expiring it...", &p, e);
                    if let Err(e) = self.delete(&p).await {
                        log::debug!("Delete failed on object '{:?}' that doesn't respond to head: {}", p, e);
                    }
                }
            }
            Ok(()) // squash the error and continue
        }).await
    }

}
