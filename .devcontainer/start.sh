#!/bin/bash -xe

# Fix ownership of volume mounted directories
sudo chown -R vscode:vscode /home/vscode/.aws /usr/local/cargo/registry /usr/local/cargo/git /usr/local/rustup /workspaces/archodex-backend/target

# Install / update SurrealDB CLI
curl -sSf https://install.surrealdb.com | sudo sh