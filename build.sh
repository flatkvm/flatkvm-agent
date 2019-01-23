#!/bin/sh

function create_container_image {
	CTR=$(buildah from alpine:edge)
	if [ -z "${CTR}" ]; then
		echo "Error creating container base"
		exit -1
	fi
	
	buildah run ${CTR} apk add rust cargo dbus-dev eudev-dev python3 libxcb-dev
	if [ $? != 0 ]; then
		echo "Error installing packages"
		exit -1
	fi
	
	buildah commit ${CTR} alpine-flatkvm
	if [ $? != 0 ]; then
		echo "Error commiting container image"
		exit -1
	fi

	echo "Container image successfully built"
}

podman images | grep alpine-flatkvm &> /dev/null
if [ $? != 0 ]; then
	echo "Can't find alpine-flatkvm container image, creating it..."
	create_container_image
fi

echo "Starting a container to run \"cargo\""
podman run -v $(pwd):/workdir:Z -ti --rm -w /workdir alpine-flatkvm cargo build --release
