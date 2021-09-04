OBJS_UNDER_GIT=`git ls-files | grep '\.o'`
if [ "${OBJS_UNDER_GIT}" != "" ]; then
	echo "Some object files (.o) are under version control."
	echo "Please remove them.";
	echo ${OBJS_UNDER_GIT} | tr ' ' '\n';
	exit 1;
fi
