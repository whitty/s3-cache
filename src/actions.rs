// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use anyhow::Context;
use async_std::path::PathBuf;

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
    log::debug!("Fetching metadata for {:?}", &path);

    let mut m = Meta::new(path);
    m.resolve().await?;

    if m.file.as_ref().map_or(true, std::fs::Metadata::is_file) {
        m.hash = Some(cache::read_hash(m.path.as_path(), &m.file.as_ref().map(std::fs::Metadata::len)).await?);
    }
    Ok(m)
}

async fn download_file(storage: Storage, file: cache::File, cache_name: String, base: PathBuf) -> Result<()> {
    let mut base = base;
    base.push(&file.path);

    if let Some(p) = base.parent() {
        if p != base && ! p.is_dir().await {
            log::info!("creating directory {:?} for {:?}", &p, &base);
            std::fs::create_dir_all(p)?;
        }
    }

    let mut f = tokio::fs::File::create(&base).await?;

    let p = file.storage_path(cache_name.as_str());
    let object_path = p.to_str().expect("Invalid storage_path -> string");
    log::debug!("Downloading {:?} from {}", base, object_path);
    storage.get_file(&mut f, object_path).await?;
    Ok(())
}

async fn upload_file(storage: Storage, file: cache::File, cache_name: String) -> Result<()> {
    let mut f = tokio::fs::File::open(&file.path).await?;

    let p = file.storage_path(cache_name.as_str());
    let path = p.to_str().expect("Invalid storage_path -> string");
    log::info!("Inserting {}", file.path);
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

async fn work_upload(storage: Storage, file: cache::File, cache_name: String) -> UploadWork {
    UploadWork::Upload(upload_file(storage, file, cache_name).await)
}

pub async fn expire(storage: Storage, age_days: u32) -> Result<()> {
    let now = chrono::Utc::now();
    let expiry_time = now.checked_sub_days(
        chrono::Days::new(age_days as u64))
        .ok_or(crate::Error::ExpiryAgeConversionError(age_days))?;

    storage.recursive_expire("objects/", expiry_time).await?;
    Ok(())
}

pub async fn upload(storage: Storage,
                    cache_name: &str, paths: &[std::path::PathBuf],
                    cache_threshold: usize) -> Result<()> {

    let mut path_set = tokio::task::JoinSet::<UploadWork>::new();

    for path in paths {
        path_set.spawn(work_meta_for(path.into()));
    }

    let mut cache_entry = cache::Cache::default();

    log::debug!("Dispatching upload processing jobs...");
    while let Some(work) = path_set.join_next().await {
        // JoinError
        let work = work.with_context(|| "Failure waiting on upload work")?;

        match work {
            UploadWork::Meta(meta) => {
                let meta = meta.with_context(|| "Failed to load metadata")?;

                log::debug!("{:?}\tmeta={:?} size={:?} path={:?}",
                            meta.path.to_str(), meta, meta.file.as_ref().map_or(0, |x| { x.len() }),
                            meta.object_path());

                if !meta.is_cacheable() {
                    continue;
                }

                let path = meta.path.to_str().expect("bad paths should be handled by is_cacheable");
                let object = meta.object_path().expect("todo no path should be handled by is_cacheable").to_str().expect("should not generate bad paths").to_owned();
                let size = meta.file.as_ref().map_or(0, std::fs::Metadata::len);

                // small files should be uploaded under cache and not deduped for deletion
                // pragmatism
                let object = if size > cache_threshold.try_into().expect("usize should if in u64") {
                    Some(object.clone())
                } else {
                    None
                };

                let file = cache::File {
                    path: path.to_owned(),
                    object,
                    size
                };

                cache_entry.files.push(file.clone());

                path_set.spawn(work_upload(storage.clone(), file, cache_name.to_owned()));
            },

            UploadWork::Upload(result) => {
                result.with_context(|| "Failed to upload file")?;
            },
        }
    }

    let path = Cache::entry_location(cache_name);
    log::debug!("Pushing cache entry with {} files to {:?}", cache_entry.files.len(), path);
    storage.put_file(&mut std::io::Cursor::new(cache_entry.into_string()), path.to_str().unwrap()).await?;

    Ok(())
}

async fn read_cache_info(storage: &Storage, cache_name: &str) -> Result<Cache> {
    let path = Cache::entry_location(cache_name);

    let mut vec = Vec::<u8>::new();
    storage.get_file(&mut vec, path.to_str().unwrap()).await?;
    let c = cache::decode(&vec)?;
    Ok(c)
}

pub async fn list(storage: Storage, cache_name: Option<&str>) -> Result<()> {
    if let Some(cache_name) = cache_name {
        let c = read_cache_info(&storage, cache_name).await?;

        let largest = c.files.iter().max_by(|x, y| x.path.len().cmp(&y.path.len()));
        if let Some(longest) = largest {
            let len = longest.path.len().max(30);
            for f in c.files {
                println!("{path:<0$} {size:>10}", len, path=f.path, size=f.size);
            }
        }
    } else {
        for c in storage.list_dirs("cache/").await? {
            println!("{}", c);
        }
    }
    Ok(())
}

enum DownloadWork {
    Download(Result<()>)
}

async fn work_download(storage: Storage, file: cache::File, cache_name: String, base: PathBuf) -> DownloadWork {
    DownloadWork::Download(download_file(storage, file, cache_name, base).await)
}

pub async fn download(storage: Storage, cache_name: &str, outpath: std::path::PathBuf) -> Result<()> {
    let c = read_cache_info(&storage, cache_name).await?;
    if ! c.files.is_empty() && !outpath.is_dir() {
        std::fs::create_dir_all(&outpath).context(format!("Failed to create {:?}", &outpath))?;
    }

    let mut download_set = tokio::task::JoinSet::<DownloadWork>::new();
    for f in c.files {
        download_set.spawn(work_download(storage.clone(), f.clone(), cache_name.to_owned(), outpath.clone().into()));
    }

    log::debug!("Dispatching download jobs...");
    while let Some(work) = download_set.join_next().await {
        // JoinError
        let work = work.with_context(|| "Failure waiting on download jobs")?;

        match work {
            DownloadWork::Download(result) => {
                result.with_context(|| "Failed to download file")?;
            }
        }
    }

    Ok(())
}

pub async fn delete(storage: Storage, cache_name: &str) -> Result<()> {
    if let Err(e) = read_cache_info(&storage, cache_name).await {
        log::warn!("Cache {} not found:{}", cache_name, e);
    }

    let mut path = Cache::entry_location(cache_name);
    path.pop();
    storage.recursive_delete_p(path.as_ref()).await?;
    Ok(())
}
