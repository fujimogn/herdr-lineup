#!/bin/sh
# Reject a pushed v<semver> tag whose version files differ from the tag or
# whose commit is not on main. Other refs pass through.
# This is a script, not a command: lefthook skips pre-push commands when the
# push changes no files, which is the case for a tag-only push.
for name in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$name"; done
exec .develop-tools/resolve pre-push check-release-version --pre-push
