# wasabi-builder

Dockerfile for building & testing Wasabi OS.

## Updating the builder image

```
make # follow the instructions printed by the command to upload
```

## Raw commands (for debugging)

```
docker build .
docker build --no-cache .
docker run -it <image id>
```
