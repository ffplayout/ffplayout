#!/bin/bash

input=$1

if [[ "$input" == "start" ]]; then
    sudo /bin/systemctl start srs
    sleep 2
    sudo /bin/systemctl start ffplayout
elif [[ "$input" == "stop" ]]; then
    sudo /bin/systemctl stop srs
    sleep 2
    sudo /bin/systemctl stop ffplayout
fi
