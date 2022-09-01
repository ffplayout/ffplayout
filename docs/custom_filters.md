## Custom filter

ffplayout allows it to define a custom filter string. For that is the parameter **custom_filter** in the **ffplayout.yml** config file under **processing**. The playlist can also contain a **custom_filter** parameter for every clip, with the same usage.

The filter outputs should end with `[c_v_out]` for video filter, and `[c_a_out]` for audio filter. The filters will be apply on every clip and after the filters which unify the clips.

It is possible to apply only video or audio filters, or both. For a better understanding here some examples:

#### Apply Gaussian blur and volume filter:

```YAML
custom_filter: 'gblur=5[c_v_out];volume=0.5[c_a_out]'
```

#### Add lower third:

```YAML
custom_filter: '[v_in];movie=/path/to/lower_third.png:loop=0,scale=1024:576,setpts=N/(25*TB)[lower];[v_in][lower]overlay=0:0:shortest=1[c_v_out]'
```

Pay attention to the filter prefix `[v_in];`, this is necessary to get the output from the regular filters.

#### Paint effect

```YAML
custom_filter: edgedetect=mode=colormix:high=0[c_v_out]
```

Check ffmpeg [filters](https://ffmpeg.org/ffmpeg-filters.html) documentation, and find out which other filters ffmpeg has.

