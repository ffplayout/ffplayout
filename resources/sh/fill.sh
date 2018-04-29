#!/bin/bash

export LC_ALL=en_US.utf8
filler_path=$( awk -F' = ' '/^filler_path/{ print $2  }' /etc/ffplayout/ffplayout.conf )
filler=$( awk -F' = ' '/^filler_clip/{ print $2  }' /etc/ffplayout/ffplayout.conf )
filler_dur=$( ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$filler" )

date=$1
diff=$2
start=$3

list=''

while read -r file; do
    dur=$( ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$file" )
    if (( $(bc <<< "$diff-$dur>170") )); then
        name=$( echo "$file" | sed 's/&/&amp;/g' )
        list+=$( printf '        <video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>%s' "$name" "$start" "$dur" "0.0" "$dur" "\n" )

        start=$( echo "$start + $dur" | bc )
        diff=$( echo "$diff - $dur" | bc )
    elif (( $(bc <<< "$diff<=$dur+5 && $diff>=$dur") )); then
        name=$( echo "$file" | sed 's/&/&amp;/g' )
        list+=$( printf '        <video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>%s' "$name" "$start" "$dur" "0.0" "$dur" "\n" )

        start=$( echo "$start + $dur" | bc )
        diff=$( echo "$diff - $dur" | bc )
        break
    fi
done < <( find "$filler_path" -type f | sort -R )

if (( $(bc <<< "$diff>10") )); then
    while read -r file; do
        dur=$( ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$file" )
        if (( $(bc <<< "$diff<=$dur+5 && $diff>=$dur") )); then
            name=$( echo "$file" | sed 's/&/&amp;/g' )
            list+=$( printf '        <video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>%s' "$name" "$start" "$dur" "0.0" "$dur" "\n" )

            start=$( echo "$start + $dur" | bc )
            diff=$( echo "$diff - $dur" | bc )
            break
        fi
    done < <( find "$filler_path" -type f | sort -R )
fi

seek=$( echo "$filler_dur - $diff" | bc )
list+=$( printf '        <video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>%s' "$filler" "$start" "$filler_dur" "$seek" "$filler_dur" "\n" )

printf "$list"
