#!/usr/bin/bash

input=$1
output=$2
print_block=false

if [ ! "$input" ] || [ ! "$output" ]; then
    echo "Run script like: den_doc.sh input.rs output.md"
fi

:> "$output"

while IFS= read -r line; do
    if echo $line | grep -Eq  "^///"; then
        echo "$line" | sed -E "s|^/// ?||g" >> "$output"
        print_block=true
    fi

    if [ -z "$line" ] && [[ $print_block == true ]]; then
        echo "" >> "$output"
        print_block=false
    fi
done < "$input"
