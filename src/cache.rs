// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use std::path::PathBuf;

use super::Result;
use sha2::{Sha256, Digest};
use tokio::io::AsyncReadExt;
use path_slash::PathExt as _;
use path_slash::PathBufExt as _;

use serde::{Deserialize, Serialize};

/// Wrap cache entries in version number we so we can be incompatible later
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) enum CacheVersions {
    #[serde(rename = "v1")]
    V1(Cache),
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub(crate) struct Cache {
    pub files: Vec<File>,
}

impl Cache {
    pub fn entry_location(cache_name: &str) -> PathBuf {
        let mut b = Self::location(cache_name);
        b.push("entry");
        PathBuf::from(b.to_slash().expect("slash conversion").as_ref())
    }

    pub fn location(cache_name: &str) -> PathBuf {
        let mut b = PathBuf::new();
        b.push("cache");
        b.push(cache_name);
        PathBuf::from(b.to_slash().expect("slash conversion").as_ref())
    }

    pub fn into_string(self) -> String {
        let cache = CacheVersions::V1(self);
        serde_json::to_string(&cache).expect("Cache entries should be serialiseable")
    }
}

pub(crate) fn decode(v: &[u8]) -> Result<Cache> {
    let x: CacheVersions = serde_json::from_str(std::str::from_utf8(v)?)?;
    match x {
        CacheVersions::V1(c) => Ok(c),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct File {
    path: String,
    pub object: Option<String>,
    pub size: u64,
    pub mode: Option<u32>,
    pub link_target: Option<String>,
}

impl File {
    fn new(path: &std::path::Path, object: Option<std::path::PathBuf>, size: u64, mode: Option<u32>, link_target: Option<String>) -> File {
        File {
            path: path.to_slash().expect("path->slash").to_string(),
            object: object.map(|x| x.to_slash().expect("path->slash").to_string()),
            size,
            mode,
            link_target
        }
    }

    // Massage entry into slash format
    pub fn new_async(path: &async_std::path::Path, object: Option<async_std::path::PathBuf>, size: u64, mode: Option<u32>, link_target: Option<String>) -> File {
        Self::new(
            std::path::PathBuf::from(path.as_os_str()).as_path(),
            object.map(|x| std::path::PathBuf::from(x.as_path())),
            size,
            mode,
            link_target
        )
    }

    pub fn path_str(&self) -> &str {
        self.path.as_str()
    }

    pub fn path(&self) -> PathBuf {
        PathBuf::from_slash(self.path.as_str())
    }

    pub fn storage_path(&self, cache_name: &str) -> PathBuf {
        let mut b = PathBuf::new();
        if let Some(s) = self.object.as_ref() {
            b.push("objects");
            b.push(s);
            b.push("bin");
        } else {
            b.push("cache");
            b.push(cache_name);
            b.push("files");
            b.push(&self.path);
        }
        PathBuf::from(b.to_slash().expect("slash conversion").as_ref())
    }
}

pub(crate) async fn read_hash(path: &async_std::path::Path, len: &Option<u64>) -> Result<[u8;32]> {

    // allocate a buffer one page -> 1 meg
    let buf_size = len.unwrap_or(0).clamp(4096, 1024*1024);
    let mut buf = vec![0; buf_size.try_into().unwrap()];
    let mut sha = Sha256::new();

    let mut f = tokio::fs::File::open(path).await?;
    loop {
        let len = f.read(&mut buf).await?;
        if len == 0 { break; }
        sha.update(&buf[..len]);
    }
    let result = sha.finalize();
    Ok(result.into())
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn version_compat() {
        let cache: Cache = serde_json::from_str("{ \"files\": [] }").unwrap();
        assert!(cache.files.is_empty());
        let cache: Cache = serde_json::from_str(r#"{ "files": [], "else": 1 }"#).unwrap();
        assert!(cache.files.is_empty(), "Expected to ignore additional fields");

        // Round trip of version container
        let mut c = Cache::default();
        c.files.push(File{ path: "foo.exe".into(), object: Some("aa/bb/cc/dddd".into()), size: 123456, mode: Some(0o100664), link_target: None });
        c.files.push(File{ path: "libfoo.so".into(), object: None, size: 7, mode: None, link_target: Some("libfoo.so.1".into()) });
        let v = CacheVersions::V1(c);
        let x = serde_json::to_string(&v).unwrap();
        println!("json = {}", x);

        let inp: CacheVersions = serde_json::from_str(r#" {
"v1": {
  "files": [
    {"path":"foo.exe","object":"aa/bb/cc/dddd","size":123456,"mode":33204},
    {"path":"libfoo.so","size":7,"link_target": "libfoo.so.1"}
  ]
}
}"#).unwrap();

        assert_eq!(inp, v);
        assert_eq!(serde_json::from_str::<CacheVersions>(&x).unwrap(), v);
    }

    // construct a path-like string from directory and file
    // This is to pass windows\directories on windows
    fn path_str(d: &str, f: &str) -> String {
        let mut p = PathBuf::from(d);
        p.push(f);
        String::from(p.to_str().expect("only valid path strings"))
    }

    fn file_path_with_object() -> File {
        File::new(PathBuf::from(path_str("dir", "file")).as_path(),
                  Some(PathBuf::from(path_str("dir2", "file2"))),
                  100, Some(0), None)
    }

    fn file_path() -> File {
        let mut f = file_path_with_object();
        f.object = None;
        f
    }

    #[test]
    fn cache_file_path_compat() {
        let f = file_path();

        // should match directory specific
        assert_eq!(f.path(), PathBuf::from(path_str("dir", "file")));

        #[cfg(not(unix))]
        assert_ne!(f.path().to_str().unwrap(), "dir/file", "expected windows path");
    }

    #[test]
    fn cache_file_path_str_compat() {
        assert_eq!(file_path().path_str(), "dir/file");
    }

    #[test]
    fn cache_file_object_path_compat() {
        // should be / even on windows
        assert_eq!(file_path_with_object().object.as_ref().expect("must not be none"), "dir2/file2");
    }

    #[test]
    fn cache_file_object_storage_compat() {
        assert_eq!(file_path_with_object().storage_path("mycache").to_str().expect("valid string"), "objects/dir2/file2/bin");
    }

    #[test]
    fn cache_file_storage_compat() {
        assert_eq!(file_path().storage_path("mycache").to_str().expect("valid string"), "cache/mycache/files/dir/file");
    }

    #[test]
    fn path_compat_entry_location() {
        assert_eq!(Cache::entry_location("mycache").to_str().expect("valid string"), "cache/mycache/entry");
    }

    #[test]
    fn path_compat_location() {
        assert_eq!(Cache::location("mycache").to_str().expect("valid string"), "cache/mycache");
    }
}
