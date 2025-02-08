// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use anyhow::Context;
use s3::Bucket;

use crate::Result;

#[derive(Debug)]
struct Meta {
    path: String,
}

async fn meta_for(path: String) -> Result<Meta> {
    println!("path={}", path);
    Ok(Meta{path})
}

pub async fn upload(_bucket: Box<Bucket>,
                    _name: &str, paths: &[String] ) -> Result<()> {

    let mut path_set = tokio::task::JoinSet::<Result<Meta>>::new();

    for path in paths {
        path_set.spawn(meta_for(path.clone()));
    }

    let mut output = Vec::new();
    while let Some(meta) = path_set.join_next().await {
        // JoinError
        let meta = meta.with_context(|| "Failure waiting on metadata parsing")?;
        // Our Errors
        let meta = meta.with_context(|| "Failed to load metadata")?;
        println!("{}\tmeta={:?}", meta.path, meta);
        output.push(meta);
    }

    Ok(())
}
