# Custom Configuration

Extend your arguments for using them in your custom extensions.

The file name must have the **argparse_** prefix. The content should look like:

```YAML
short: -v
long: --volume
help: set audio volume
```

At least **short** or **long** have to exist, all other parameters are optional. You can also extend the config, with keys which are exist in **ArgumentParser.add_argument()**.

**Every argument must have its own yaml file!**
