#!/bin/sh

temp_file=$(mktemp /tmp/$(basename $0)-release-notes.XXXXXX)
version=$(cat VERSION)

cat<<EOF >${temp_file}
#
# Note:
#     - lines starting with a '#' are ignored
#     - the first line not starting with a '#' is the title ... don't leave an extra blank line!
#     - Markdown is _not_ rendered on Github, the release text is formatted as <pre> text
#     - release version will be ${version}
#     - you can abort release after exiting this editor
#
# Release title (this will be prefixed by github with '${version}): '

# Release description


EOF

$EDITOR ${temp_file}

read -n1 -p "continue creating release ${version}? (y/n)? "
echo

if [ "$REPLY" = "y" ]
then
    git tag --file ${temp_file} ${version}
    git push --tags
else
    echo "aborting"
fi
