// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use super::Result;
use sha2::{Sha256, Digest};
use tokio::io::AsyncReadExt;

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
