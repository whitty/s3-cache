// SPDX-License-Identifier: GPL-3.0-or-later
// (C) Copyright 2025 Greg Whiteley

use s3::Bucket;

use crate::Result;

pub async fn upload(_bucket: Box<Bucket>) -> Result<()> {
    Ok(())
}
