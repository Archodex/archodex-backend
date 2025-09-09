#!/bin/bash -xe

DEVCONTAINER_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

# Run common container start script
. "$DEVCONTAINER_DIR/start.sh"

# Don't remove apt cache after installing packages
sudo rm /etc/apt/apt.conf.d/docker-clean

sudo apt-get update
sudo apt-get install -y --no-install-recommends clang jq mold protobuf-compiler

rustup component add clippy rustfmt

# Install act
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/nektos/act/master/install.sh | sudo bash -s -- -b /usr/bin