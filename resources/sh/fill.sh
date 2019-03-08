#!/bin/bash

export LC_ALL=en_US.utf8
filler_path=$( awk -F' = ' '/^filler_path/{ print $2  }' /etc/ffplayout/ffplayout.conf )
filler=$( awk -F' = ' '/^filler_clip/{ print $2  }' /etc/ffplayout/ffplayout.conf )
filler_dur=$( ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$filler" )

diff=$1

list=''

while read -r file; do
    dur=$( ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$file" )
    if (( $(bc <<< "$diff-$dur>170") )); then
        list+="0.0|$dur|$dur|$file\n"

        diff=$( echo "$diff - $dur" | bc )
    elif (( $(bc <<< "$diff<=$dur+5 && $diff>=$dur") )); then
        list+="0.0|$dur|$dur|$file\n"

        diff=$( echo "$diff - $dur" | bc )
        break
    fi
done < <( find "$filler_path" -type f | sort -R )

if (( $(bc <<< "$diff>10") )); then
    while read -r file; do
        dur=$( ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$file" )
        if (( $(bc <<< "$diff<=$dur+5 && $diff>=$dur") )); then
            list+="0.0|$dur|$dur|$file\n"

            diff=$( echo "$diff - $dur" | bc )
            break
        fi
    done < <( find "$filler_path" -type f | sort -R )
fi

seek=$( echo "$filler_dur - $diff" | bc )
list+="$seek|$filler_dur|$filler_dur|$filler\n"

printf "$list"
