SupervisorD
-----

The supervisor config is only needed when you want to run multiple channels.

Every channel has his own config in [conf.d](/supervisor/conf.d/) folder. In the configuration you have to change this line:

```
command=./venv/bin/python3 ffplayout.py -c /etc/ffplayout/ffplayout-001.yml
```
to the correct ffpalyout YAML config file.
