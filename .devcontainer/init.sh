#!/bin/bash -xe

sudo apt-get update
sudo apt-get install -y clang jq mold protobuf-compiler

rustup component add clippy rustfmt

# Install act
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/nektos/act/master/install.sh | sudo bash -s -- -b /usr/bin