NAME=wasabi-builder
TAG_TO_BUILD:=latest
IMAGE_TAG:=hikalium/${NAME}:$(TAG_TO_BUILD)

.PHONY : build
build:
	docker build -t ${IMAGE_TAG} .
	@ echo ${IMAGE_TAG}
	@ echo Run \"docker push ${IMAGE_TAG}\" to upload this image.
