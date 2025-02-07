// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Cache named '{0}' not found")]
    CacheNotFound(String),

    #[error("Bucket named '{0}' not found, and create not allowed")]
    BucketNotFound(String),

    // REVIEW
    #[error("I/O error reading from socket: {0}")]
    IO(#[from] std::io::Error),
}
