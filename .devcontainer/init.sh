#!/bin/bash -xe

DEVCONTAINER_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "$DEVCONTAINER_DIR/.."

apt-get update
apt-get install -y clang jq protobuf-compiler

rustup component add clippy rustfmt

# Install zig
pushd "$HOME"
curl -LO https://ziglang.org/builds/zig-linux-aarch64-0.15.0-dev.386+2e35fdd03.tar.xz
tar -xf zig-linux-aarch64-*.tar.xz
rm zig-linux-aarch64-*.tar.xz
mv zig-* zig
popd

# Install cargo-lambda
export PATH="${PATH}:${HOME}/zig"
curl -fsSL https://cargo-lambda.info/install.sh | sh

# Install act
pushd "$HOME"
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/nektos/act/master/install.sh | bash
popd

# Install latest mold, debian bookworm is very out of date
# Check if mold is already installed
if ! command -v mold &> /dev/null; then
    echo "Mold not found. Installing latest version..."
    mold_url=$(curl -s https://api.github.com/repos/rui314/mold/releases/latest | jq -r ".assets | map(select(.name | test(\"`uname -m`\"))) | .[0].browser_download_url")
    curl -L "$mold_url" > /tmp/mold.tar.gz
    tar -C /usr --strip-components=1 -xzf /tmp/mold.tar.gz
    rm /tmp/mold.tar.gz
    echo "Mold installed successfully."
    
    # Use mold by default when building rust executables
    if [ -d "${CARGO_HOME}" ]; then
        if ! grep -q "link-arg=-fuse-ld=mold" "${CARGO_HOME}/config.toml" 2>/dev/null; then
            echo "Configuring Cargo to use mold..."
            cat << EOF >> "${CARGO_HOME}/config.toml"
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]

[target.aarch64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
EOF
        fi
    fi
else
    echo "Mold is already installed. Version: $(mold --version 2>&1 | head -n 1)"
fi

code --install-extension vadimcn.vscode-lldb
code --install-extension github.vscode-github-actions
code --install-extension ms-vscode.cpptools
code --install-extension rust-lang.rust-analyzer
code --install-extension xaver.clang-format

# Install AWS CLI v2
# Check if AWS CLI is already installed
if ! command -v aws &> /dev/null; then
    echo "AWS CLI not found. Installing..."
    curl "https://awscli.amazonaws.com/awscli-exe-linux-aarch64.zip" -o "awscliv2.zip"
    unzip -q awscliv2.zip
    ./aws/install
    rm -rf awscliv2.zip aws
    echo "AWS CLI installed successfully."
else
    echo "AWS CLI is already installed. Version: $(aws --version)"
fi

export COLORTERM=truecolor
export AWS_REGION=us-west-2
export AWS_ACCESS_KEY_ID=invalid
export AWS_SECRET_ACCESS_KEY=invalid
export AWS_ENDPOINT_URL=http://localhost:8001
export AWS_PAGER=""

# Ensure the DynamoDB accounts table exists
if ! aws dynamodb describe-table --table-name archodex-accounts 2>/dev/null; then
    echo "Creating archodex-accounts table..."
    aws dynamodb create-table \
        --table-name archodex-accounts \
        --attribute-definitions AttributeName=pk,AttributeType=B AttributeName=sk,AttributeType=B \
        --key-schema AttributeName=pk,KeyType=HASH AttributeName=sk,KeyType=RANGE \
        --billing-mode PAY_PER_REQUEST
else
    echo "Table archodex-accounts already exists."
fi

# Run the migrator in local dev mode
echo "Building and running migrator in local dev mode..."
cargo build --bin=migrator --package=migrator

# Run the migrator binary
echo "Migrating accounts table..."
"./target/debug/migrator"
echo "Migration completed successfully."
