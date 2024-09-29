#!/bin/bash -e
PROJ_ROOT="$(dirname $(dirname ${BASH_SOURCE:-$0}))"
cd "${PROJ_ROOT}"

# Download the Change-Id hook if it does not exist.
wget -O .git/hooks/commit-msg -nc https://raw.githubusercontent.com/GerritCodeReview/gerrit/d5403dbf335ba7d48977fc95170c3f7027c34659/resources/com/google/gerrit/server/tools/root/hooks/commit-msg && chmod 755 .git/hooks/commit-msg || true

# If there are some commits without a Change-Id, rebase all commits to put it.
[ "$(git log --pretty=one --grep=^Change-Id: | wc -l)" == "$(git log --pretty=one | wc -l)" ] \
	&& echo "Found Change-Id: on all commits" \
	|| GIT_SEQUENCE_EDITOR=: git rebase --root -i --autosquash --exec 'EDITOR=: git commit --amend'

# Run tests over all commits unless the commit has "SKIP_TEST:" tag in the commit title.
git rebase --root --exec "git show --oneline -s HEAD | grep SKIP_TEST: || cargo test"
