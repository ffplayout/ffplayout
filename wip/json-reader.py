#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import json

with open('2019-03-05.json') as f:
    data = json.load(f)

h, m, s = data["begin"].split(':')
begin = float(h) * 3600 + float(m) * 60 + float(s)

for item in data["program"]:
    inp = item["in"]
    out = item["out"]
    duration = item["duration"]

    print(begin)
    begin += out - inp
