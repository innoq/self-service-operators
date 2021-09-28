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

cat<<'EOF'

                 _       _
 _   _ _ __   __| | __ _| |_ ___    __ _ _ __  _ __
| | | | '_ \ / _` |/ _` | __/ _ \  / _` | '_ \| '_ \
| |_| | |_) | (_| | (_| | ||  __/ | (_| | |_) | |_) |
 \__,_| .__/ \__,_|\__,_|\__\___|  \__,_| .__/| .__/
      |_|                               |_|   |_|
                    _                                   _
__   _____ _ __ ___(_) ___  _ __    _ __   _____      _| |
\ \ / / _ \ '__/ __| |/ _ \| '_ \  | '_ \ / _ \ \ /\ / / |
 \ V /  __/ |  \__ \ | (_) | | | | | | | | (_) \ V  V /|_|
  \_/ \___|_|  |___/_|\___/|_| |_| |_| |_|\___/ \_/\_/ (_)

EOF
