# wasabi-builder

Dockerfile for building & testing Wasabi OS and Browser.

```
docker build .
docker build --no-cache .
docker run -it <image id>

export IMAGE_TAG=v`date +%Y%m%d_%H%M%S` && echo ${IMAGE_TAG}
docker tag <image id> hikalium/wasabi-builder:${IMAGE_TAG}
docker push hikalium/wasabi-builder:${IMAGE_TAG}
```
