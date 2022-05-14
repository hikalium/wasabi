NAME=wasabi-builder
# TAG_TO_BUILD:=v$(shell date +%Y%m%d_%H%M%S)
TAG_TO_BUILD:=latest
IMAGE_TAG:=hikalium/${NAME}:$(TAG_TO_BUILD)

build:
	docker build -t ${IMAGE_TAG} .
	@ echo
	@ echo ${IMAGE_TAG}
	@ echo Run \"docker push ${IMAGE_TAG}\" to upload this image.
	@ echo Also, please make sure to update .circleci/config.yml.
