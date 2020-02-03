#!/bin/bash

# This file is part of ffplayout.
#
# ffplayout is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# ffplayout is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with ffplayout. If not, see <http://www.gnu.org/licenses/>.

# ------------------------------------------------------------------------------

src=$1

listDate=$(date +%Y-%m-%d)

trunk="/playlists/$(date +%Y)/$(date +%m)/"
playlist="$listDate.json"

[[ -d "$trunk" ]] || mkdir -p "$trunk"

c="0"
count=$( find "$src" -name "*.mp4" | wc -l )

# build Head for playlist
printf  '{
	"channel": "Test 1",
	"date": "%s",
	"program": [{\n' $listDate > "$trunk/$playlist"

# read playlist
while read -r line; do
	clipPath=$(echo "$line" | sed 's/&/&amp;/g')
	clipDuration=$( ffprobe -v error -show_format  "$line" | awk -F= '/duration/{ print $2 }' )

	c=$((c + 1))

	if (( c < count )); then
		last="}, {"
	else
		last="}]"
	fi

	printf '\t\t"in": 0,\n\t\t"out": %s,\n\t\t"duration": %s,\n\t\t"source": "%s"\n\t%s\n' "$clipDuration" "$clipDuration" "$clipPath" "$last" >> "$trunk/$playlist"

done < <( find "$src" -name "*.mp4" | sort -R)

printf "}\n" >> "$trunk/$playlist"
