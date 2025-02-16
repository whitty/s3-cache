// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Cache named '{0}' not found")]
    CacheNotFound(String),

    #[error("Bucket named '{0}' not found, and create not allowed")]
    BucketNotFound(String),

    #[error("Unable to respresent path '{0}'")]
    InvalidPath(std::path::PathBuf),

    #[error("S3 Credential error: {0}")]
    S3CredentialsError(#[from] s3::creds::error::CredentialsError),

    #[error("Error from S3 service: {0}")]
    S3Error(#[from] s3::error::S3Error),

    #[error("Error creating bucket: {0}")]
    BucketCreationError(s3::error::S3Error),

    #[error("I'm lazy, you'll never see this")]
    OptionWasNoneError,

    #[error("Time parse error: {0}")]
    DateTimeParseError(chrono::ParseError),

    #[error("Unable to determine expiry time from {0} days")]
    ExpiryAgeConversionError(u32),

}
