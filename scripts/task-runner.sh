#!/usr/bin/bash

# media object
mObj=$1

# perform a meaningful task
notify-send -u normal "ffplayout" -t 2 -e "Play: $(echo $mObj | jq -r '.current_media.source')"
