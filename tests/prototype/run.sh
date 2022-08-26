#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

PLATFORM=$(uname -sp)
if [[ $PLATFORM != "Linux x86_64" ]]
then
  echo
  echo "Prototype only works on Linux, skipping..."
  echo
  exit 0
fi

rm -rf /tmp/check_format
cp -r -L check_format /tmp/
cp -r -L ../../library /tmp/
cd /tmp/check_format

time_env="env time -v"
$time_env cargo kani --enable-unstable --ignore-global-asm --harness check_format
