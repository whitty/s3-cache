// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use anyhow::Context;
use async_std::path::PathBuf;
use s3::Bucket;
use sha2::{Sha256, Digest};
use tokio::{fs::File, io::AsyncReadExt};

use crate::Result;

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

    fn path(&self) -> Option<PathBuf> {
        self.hash.map(|ref x| {
            let mut path = PathBuf::new();
            path.push(faster_hex::hex_string(&x[0..4]));
            path.push(faster_hex::hex_string(&x[4..8]));
            path.push(faster_hex::hex_string(&x[8..12]));
            path.push(faster_hex::hex_string(&x[12..]));
            path
        })
    }
}

async fn meta_for(path: PathBuf) -> Result<Meta> {
    println!("path={:?}", path);
    let mut m = Meta::new(path);
    m.resolve().await?;

    if m.file.as_ref().map_or(true, std::fs::Metadata::is_file) {
        // allocate a buffer one page -> 1 meg
        let buf_size = m.file.as_ref().map_or(0, std::fs::Metadata::len).clamp(4096, 1024*1024);
        let mut buf = vec![0; buf_size.try_into().unwrap()];
        let mut sha = Sha256::new();

        let mut f = File::open(&m.path).await?;
        loop {
            let len = f.read(&mut buf).await?;
            if len == 0 { break; }
            sha.update(&buf[..len]);
        }
        let result = sha.finalize();
        m.hash = Some(result.into());
    }

    Ok(m)
}

pub async fn upload(_bucket: Box<Bucket>,
                    _name: &str, paths: &[std::path::PathBuf] ) -> Result<()> {

    let mut path_set = tokio::task::JoinSet::<Result<Meta>>::new();

    for path in paths {
        path_set.spawn(meta_for(path.into()));
    }

    let mut output = Vec::new();
    while let Some(meta) = path_set.join_next().await {
        // JoinError
        let meta = meta.with_context(|| "Failure waiting on metadata parsing")?;
        // Our Errors
        let meta = meta.with_context(|| "Failed to load metadata")?;
        println!("{:?}\tmeta={:?} size={:?} path={:?}",
                 meta.path.to_str(), meta, meta.file.as_ref().map_or(0, |x| { x.len() }),
                 meta.path());
        output.push(meta);
    }

    // would be nice to start work when the first arrives instead,...

    Ok(())
}
