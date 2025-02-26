// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use anyhow::Context;
use async_std::{fs, path::PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::{Result, cache::{self, Cache}, Storage};

#[derive(Debug)]
struct Meta {
    path: PathBuf,
    file: Option<std::fs::Metadata>,
    hash: Option<[u8;32]>,
    link_target: Option<PathBuf>,
}

impl Meta {
    fn new(path: PathBuf) -> Meta {
        Meta { path, file: None, hash: None, link_target: None }
    }

    async fn resolve(&mut self) -> Result<()> {
        self.file = Some(self.path.symlink_metadata().await?);
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

    fn cacheable_link(&self) -> Option<PathBuf> {
        self.link_target.clone()
    }

    fn is_cacheable_file(&self) -> bool {
        self.hash.is_some() && self.file.is_some()
    }

    #[cfg(unix)]
    fn get_mode(&self) -> Option<u32> {
        self.file.as_ref().map(|meta| {
            meta.permissions().mode()
        })
    }

    #[cfg(not(unix))]
    fn get_mode(&self) -> Option<u32> {
        None
    }
}

async fn meta_for(path: PathBuf) -> Result<Meta> {
    log::debug!("Fetching metadata for {:?}", &path);

    let mut m = Meta::new(path);
    m.resolve().await?;

    if m.file.as_ref().is_some_and(std::fs::Metadata::is_symlink) {
        m.link_target = Some(fs::read_link(m.path.as_path()).await?);
    }
    if m.file.as_ref().is_some_and(std::fs::Metadata::is_file) {
        m.hash = Some(cache::read_hash(m.path.as_path(), &m.file.as_ref().map(std::fs::Metadata::len)).await?);
    }
    Ok(m)
}

#[cfg(unix)]
fn create_symlink(target: String, path: PathBuf) -> Result<()> {
    log::debug!("Creating symlink {} -> {}", &path.display(), &target);
    std::os::unix::fs::symlink(target, path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink(target: String, path: PathBuf) -> Result<()> {
    log::error!("Unable to create symlink {} -> {} on Windows", &path.display(), &target);
    Ok(())
}

#[cfg(unix)]
fn set_permisions(path: &async_std::path::Path, mode: u32) {
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
        log::warn!("Failed to set permissions on {}: {}", path.to_str().unwrap(), e.kind());
    }
}

#[cfg(not(unix))]
fn set_permisions(_path: &async_std::path::Path, _mode: u32) {
}

async fn download_file(storage: Storage, file: cache::File, cache_name: String, base: PathBuf) -> Result<()> {
    let mut path = base;
    path.push(&file.path);

    if let Some(p) = path.parent() {
        if p != path && ! p.is_dir().await {
            log::info!("creating directory {:?} for {:?}", &p, &path);
            std::fs::create_dir_all(p)?;
        }
    }

    if fs::symlink_metadata(&path).await.is_ok_and(|x| x.is_symlink()) {
        // erase symlink instead of writing through it
        fs::remove_file(&path).await.context(format!("Removing existing symlink at {}", &path.display()))?;
    }

    if let Some(target) = file.link_target {
        create_symlink(target, path)?;
        return Ok(())
    }

    let mut f = tokio::fs::File::create(&path).await?;

    let p = file.storage_path(cache_name.as_str());
    let object_path = p.to_str().expect("Invalid storage_path -> string");
    log::debug!("Downloading {:?} from {}", path, object_path);
    storage.get_file(&mut f, object_path).await?;

    if let Some(mode) = file.mode {
        set_permisions(path.as_path(), mode);
    }
    Ok(())
}

async fn upload_file(storage: Storage, file: cache::File, cache_name: String, dry_run: bool) -> Result<()> {
    let mut f = tokio::fs::File::open(&file.path).await?;

    let p = file.storage_path(cache_name.as_str());
    let path = p.to_str().expect("Invalid storage_path -> string");
    log::info!("Inserting {}", file.path);
    if ! dry_run {
        storage.put_file_unless_exists(&mut f, path).await?;
    }

    Ok(())
}

enum UploadWork {
    Meta(Result<Meta>),
    Upload(Result<()>),
}

async fn work_meta_for(path: PathBuf) -> UploadWork {
    UploadWork::Meta(meta_for(path).await)
}

async fn work_upload(storage: Storage, file: cache::File, cache_name: String, dry_run: bool) -> UploadWork {
    UploadWork::Upload(upload_file(storage, file, cache_name, dry_run).await)
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
                    recurse: bool, dry_run: bool,
                    cache_threshold: usize,
                    max_in_flight: u32) -> Result<()> {

    let mut path_set = tokio::task::JoinSet::<UploadWork>::new();

    if recurse {
        for path in paths {
            for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                path_set.spawn(work_meta_for(entry.path().into()));
            }
        }
    } else {
        for path in paths {
            path_set.spawn(work_meta_for(path.into()));
        }
    }

    let mut cache_entry = cache::Cache::default();
    let mut delayed = std::collections::VecDeque::new();
    let mut net_in_flight = 0;

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

                if let Some(link) = meta.cacheable_link() {

                    let path = meta.path.to_str().expect("bad paths should be handled by is_cacheable");

                    let file = cache::File {
                        path: path.to_owned(),
                        object: None,
                        size: link.as_os_str().len() as u64,
                        mode: None,
                        link_target: Some(link.to_str().expect("symlink text should be normal string").into()),
                    };

                    cache_entry.files.push(file);

                    log::info!("{} symlink to {}", path, link.to_str().unwrap());
                    continue;
                }

                if !meta.is_cacheable_file() {
                    log::info!("{} will not be uploaded", meta.path.to_str().unwrap());
                    continue;
                }

                let path = meta.path.to_str().expect("bad paths should be handled by is_cacheable");
                let object = meta.object_path().expect("todo no path should be handled by is_cacheable").to_str().expect("should not generate bad paths").to_owned();
                let size = meta.file.as_ref().map_or(0, std::fs::Metadata::len);
                let mode = meta.get_mode();

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
                    size,
                    mode,
                    link_target: None,
                };

                cache_entry.files.push(file.clone());

                if net_in_flight >= max_in_flight {
                    delayed.push_back(work_upload(storage.clone(), file, cache_name.to_owned(), dry_run));
                } else {
                    net_in_flight += 1;
                    path_set.spawn(work_upload(storage.clone(), file, cache_name.to_owned(), dry_run));
                }
            },

            UploadWork::Upload(result) => {
                result.with_context(|| "Failed to upload file")?;
                assert!(net_in_flight > 0);
                net_in_flight -= 1;
                while !delayed.is_empty() && net_in_flight < max_in_flight {
                    net_in_flight += 1;
                    path_set.spawn(delayed.pop_front().unwrap());
                }
            },
        }
    }
    assert!(delayed.is_empty());

    let path = Cache::entry_location(cache_name);
    let count = cache_entry.files.len();
    log::debug!("Pushing cache entry with {} files to {:?}", count, path);
    if dry_run {
        log::warn!("Simulate Pushing cache entry with {} files to '{}' at {:?}", count, cache_name, path);
    } else {
        storage.put_file(&mut std::io::Cursor::new(cache_entry.into_string()), path.to_str().unwrap()).await?;
        log::warn!("Pushed {} files to '{}'", count, cache_name);
    }

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
    log::warn!("Deleted '{}'", cache_name);
    Ok(())
}
