// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use anyhow::Context;
use async_std::path::PathBuf;
use s3::Bucket;

use crate::{Result, cache::{self, Cache}, Storage};

#[derive(Debug)]
struct Meta {
    path: PathBuf,
    file: Option<std::fs::Metadata>,
    hash: Option<[u8;32]>,
}

impl Meta {
    fn new(path: PathBuf) -> Meta {
        Meta { path, file: None, hash: None }
    }

    async fn resolve(&mut self) -> Result<()> {
        self.file = Some(self.path.metadata().await?);
        Ok(())
    }

    fn object_path(&self) -> Option<PathBuf> {
        self.hash.map(|ref x| {
            let mut path = PathBuf::new();
            path.push(faster_hex::hex_string(&x[0..4]));
            path.push(faster_hex::hex_string(&x[4..8]));
            path.push(faster_hex::hex_string(&x[8..12]));
            path.push(faster_hex::hex_string(&x[12..]));
            path
        })
    }

    fn is_cacheable(&self) -> bool {
        self.hash.is_some() && self.file.is_some()
    }
}

async fn meta_for(path: PathBuf) -> Result<Meta> {
    let mut m = Meta::new(path);
    m.resolve().await?;

    if m.file.as_ref().map_or(true, std::fs::Metadata::is_file) {
        m.hash = Some(cache::read_hash(m.path.as_path(), &m.file.as_ref().map(std::fs::Metadata::len)).await?);
    }

    Ok(m)
}

async fn upload_file(storage: Storage, file: cache::File) -> Result<()> {
    let mut f = tokio::fs::File::open(&file.path).await?;

    let p = file.storage_path("TODO - name");
    let path = p.to_str().expect("Invalid storage_path -> string");
    println!("Inserting {}", file.path);
    storage.put_file_unless_exists(&mut f, path).await?;

    Ok(())
}

enum UploadWork {
    Meta(Result<Meta>),
    Upload(Result<()>),
}

async fn work_meta_for(path: PathBuf) -> UploadWork {
    UploadWork::Meta(meta_for(path).await)
}

async fn work_upload(storage: Storage, file: cache::File) -> UploadWork {
    UploadWork::Upload(upload_file(storage, file).await)
}

pub async fn expire(storage: Storage) -> Result<()> {
    // TODO - just to keep delete from being flagged unused
    storage.delete("/foo").await?;
    Ok(())
}

pub async fn upload(storage: Storage,
                    cache_name: &str, paths: &[std::path::PathBuf] ) -> Result<()> {

    let mut path_set = tokio::task::JoinSet::<UploadWork>::new();

    for path in paths {
        path_set.spawn(work_meta_for(path.into()));
    }

    let mut cache_entry = cache::Cache::default();
    while let Some(work) = path_set.join_next().await {
        // JoinError
        let work = work.with_context(|| "Failure waiting on upload work")?;

        match work {
            UploadWork::Meta(meta) => {
                let meta = meta.with_context(|| "Failed to load metadata")?;


                if !meta.is_cacheable() {
                    continue;
                }

                let path = meta.path.to_str().expect("bad paths should be handled by is_cacheable");
                let object = meta.object_path().expect("todo no path should be handled by is_cacheable").to_str().expect("should not generate bad paths").to_owned();
                let size = meta.file.as_ref().map_or(0, std::fs::Metadata::len);


                let file = cache::File {
                    path: path.to_owned(),
                    object: Some(object.clone()),
                    size
                };

                cache_entry.files.push(file.clone());

                path_set.spawn(work_upload(storage.clone(), file));
            },

            UploadWork::Upload(result) => {
                result.with_context(|| "Failed to upload file")?;
            },
        }
    }

    let path = Cache::location(cache_name);
    storage.put_file(&mut std::io::Cursor::new(cache_entry.into_string()), path.to_str().unwrap()).await?;

    // would be nice to start work when the first arrives instead,...

    Ok(())
}
