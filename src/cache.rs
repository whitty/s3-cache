// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use super::Result;
use sha2::{Sha256, Digest};
use tokio::io::AsyncReadExt;

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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct File {
    pub path: String,
    pub object: String,
    pub size: u64,
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
    fn test_compat() {
        let cache: Cache = serde_json::from_str("{ \"files\": [] }").unwrap();
        assert!(cache.files.is_empty());
        let cache: Cache = serde_json::from_str(r#"{ "files": [], "else": 1 }"#).unwrap();
        assert!(cache.files.is_empty(), "Expected to ignore additional fields");

        // Round trip of version container
        let mut c = Cache::default();
        c.files.push(File{ path: "foo.exe".into(), object: "aa/bb/cc/dddd".into(), size: 123456 });
        let v = CacheVersions::V1(c);
        let x = serde_json::to_string(&v).unwrap();

        let inp: CacheVersions = serde_json::from_str(r#" {
"v1": {
    "files":[{"path":"foo.exe","object":"aa/bb/cc/dddd","size":123456}]
  }
}"#).unwrap();
        assert_eq!(inp, v);
        assert_eq!(serde_json::from_str::<CacheVersions>(&x).unwrap(), v);
    }

}
