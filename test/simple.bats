#!/usr/bin/env bats
#
# SPDX-License-Identifier: GPL-3.0-or-later
# (C) Copyright 2025 Greg Whiteley

setup_file() {
  # ensure we have up to date build
  cargo build
}

setup() {
  # ensure executable exists
  s3_cache=$(readlink -f target/debug/s3-cache)

  test -x "$s3_cache"

  test_dir=$(mktemp -d)

  if [ -r "${TEST_DOTENV_LOCATION}" ]; then
    cp "${TEST_DOTENV_LOCATION}" "${test_dir}/.env"
  fi
  cache_name="test-$(basename "${test_dir}")"

  pushd "${test_dir}"
}

teardown() {
  echo "Cleanup delete"
  $s3_cache delete "--name=${cache_name}" || true
  popd
  [ -d "$test_dir" ] && rm -rf "$test_dir"
}

function prepare_basic_files {

  cat > hello.sh <<EOF
#!/bin/sh
echo hello world
EOF

  cat > text.txt <<EOF
This is a text file
EOF
  mkdir dir
  cat > dir/text.txt <<EOF
This is another text file
EOF

  chmod +x hello.sh
  test -x hello.sh

  echo "cache_name=$cache_name"
  find .
}

@test "basic put/get" {
  $s3_cache list

  prepare_basic_files

  $s3_cache upload --name="$cache_name" hello.sh text.txt dir/text.txt

  $s3_cache list | grep "$cache_name"

  $s3_cache download --name="$cache_name" --outpath="out"

  find .
  cmp hello.sh out/hello.sh
  cmp text.txt out/text.txt
  cmp dir/text.txt out/dir/text.txt

  test -x hello.sh
  test -x out/hello.sh

  $s3_cache delete --name="$cache_name"
}

@test "recurse put/get" {
  $s3_cache list

  prepare_basic_files

  $s3_cache upload -r --name="$cache_name" hello.sh text.txt dir

  $s3_cache list | grep "$cache_name"

  $s3_cache download --name="$cache_name" --outpath="out"

  find .
  cmp hello.sh out/hello.sh
  cmp text.txt out/text.txt
  cmp dir/text.txt out/dir/text.txt
  test -x out/hello.sh

  $s3_cache delete --name="$cache_name"
}
