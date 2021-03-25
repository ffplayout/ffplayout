# Custom Filters

Add your one filters here. They must have the correct file naming:

- for audio filter: a_[filter name].py
- for video filter: v_[filter name].py

The file itself should contain only one filter in a function named `def filter_link(prope):`

Check **v_addtext.py** for example.

In your filter you can also read custom properties from the current program node. That you can use for any usecase you wish, like reading a subtitle file, or a different logo for every clip and so on.

The normal program node looks like:

```JSON
{
    "in": 0,
    "out": 3600.162,
    "duration": 3600.162,
    "source": "/dir/input.mp4"
}
```

This you can extend to your needs, and apply this values to your filters.
