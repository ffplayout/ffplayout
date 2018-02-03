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
playlist="$listDate.xml"

# start time in seconds
listStart="21600"

[[ -d "$trunk" ]] || mkdir -p "$trunk"

# build Head for playlist
printf  '<playlist>\n\t<head>
		<meta name="author" content="example"/>
		<meta name="title" content="Live Stream"/>
		<meta name="copyright" content="(c)%s example.org"/>
		<meta name="date" content="%s"/>
	</head>\n\t<body>\n' $(date +%Y) $listDate > "$trunk/$playlist"

# read playlist
while read -r line; do
	clipPath=$(echo "$line" | sed 's/&/&amp;/g')
	clipDuration=$( ffprobe -v error -show_format  "$line" | awk -F= '/duration/{ print $2 }' )

	printf '\t\t<video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>\n' "$clipPath" "$listStart" "$clipDuration" "0.0" "$clipDuration"  >> "$trunk/$playlist"

	# add start time
	listStart="$( awk -v lS="$listStart" -v cD="$clipDuration" 'BEGIN{ print lS + cD }' )"

done < <( find "$src" -name "*.mp4" )

printf "\t</body>\n</playlist>\n" >> "$trunk/$playlist"
