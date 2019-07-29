#!/usr/bin/env bash

# Get the parent directory of where this script is.
SOURCE="${BASH_SOURCE[0]}"
while [ -h "$SOURCE" ] ; do SOURCE="$(readlink "$SOURCE")"; done
DIR="$( cd -P "$( dirname "$SOURCE" )/.." && pwd )"

# Change into that directory
cd "$DIR"

export CONTAINER_NAME=si/si-web-app
export PORT=8080:8080

echo "-------------------------------------------------"
echo "==> Launching Container: ${CONTAINER_NAME}"
echo "-------------------------------------------------"
docker run -it -p ${PORT} ${CONTAINER_NAME} 