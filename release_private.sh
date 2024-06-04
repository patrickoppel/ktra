#!/bin/bash

# This script is used to release a new version of a crate. It will:
# 
# - clone the registry repository
# - clone ktra's repository
# - setup, build and run ktra
# - login to the ktra registry with a new user
# - build and test the crate
# - publish the crate

echo "Cloning the registry repository"

cd $CRATE_PATH
ls -la
# Get registry repository from .cargo/config.toml file
REGISTRY=$(awk -F'=' '/\[registries\]/ {getline; print $1}' .cargo/config.toml | tr -d ' ')
REPO=$(grep -oP 'index = \K.*' .cargo/config.toml | tr -d '"' | awk -F/ '{print $(NF-1)"/"$NF}' | sed 's/\.git.*//')
cd ..
ls -la
eval "$(ssh-agent -s)"

# Write the private key to a file
echo $SSH_PRIV_KEY
echo $SSH_PRIV_KEY > ./private_key
chmod 600 ./private_key

# Add the private key file to the SSH agent
ssh-add ./private_key

ls -la

git clone ssh://git@github.com/$REPO

echo "Cloning ktra's repository"
git clone https://github.com/patrickoppel/ktra

echo "Setting up ktra"
echo $GITHUB_TOKEN > ktra/github_token.txt
echo "
[index_config]
remote_url = 'https://github.com/$REPO'
local_path = '../$(basename $REPO)'
ssh_privkey_path = '../private_key'
branch = 'main'
token_path = './github_token.txt'
" > ktra/ktra.toml

echo "Building ktra"
cd ktra
ls -la
cargo build --release

echo "Running ktra"
./target/release/ktra > ./ktra_output.txt 2>&1 &

echo "Waiting for ktra to start"
sleep 10
cat ./ktra_output.txt

echo "Logging in to the ktra registry"

cd $CRATE_PATH
TOKEN=$(curl -X POST -H 'Content-Type: application/json' -d '{"password":"PASSWORD"}' http://localhost:8000/ktra/api/v1/new_user/ALICE | jq -r '.token')
echo $TOKEN
cat ../ktra/ktra_output.txt

cargo login --registry=$REGISTRY $TOKEN

cargo build --release
cargo test

echo "Publish the crate"
cargo package

cat ../ktra/ktra_output.txt