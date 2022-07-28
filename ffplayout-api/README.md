**ffplayout-api**
================

ffplayout-api (ffpapi) is a non strict REST API for ffplayout. It makes it possible to control the engine, read and manipulate the config, save playlist, etc.

To be able to use the API it is necessary to initialize the settings database first. To do that, run:

```BASH
ffpapi -i
```

Then add an admin user:

```BASH
ffpapi -u <USERNAME> -p <PASSWORD> -m <MAIL ADDRESS>
```

Then run the API thru the systemd service, or like:

```BASH
ffpapi -l 127.0.0.1:8787
```

Possible Arguments
-----

```BASH
OPTIONS:
    -a, --ask                    ask for user credentials
    -d, --domain <DOMAIN>        domain name for initialization
    -h, --help                   Print help information
    -i, --init                   Initialize Database
    -l, --listen <LISTEN>        Listen on IP:PORT, like: 127.0.0.1:8787
    -m, --mail <MAIL>            Admin mail address
    -p, --password <PASSWORD>    Admin password
    -u, --username <USERNAME>    Create admin user
    -V, --version                Print version information
```

If you plan to run ffpapi with systemd set permission from **/usr/share/ffplayout** and content to user **ffpu:ffpu**. User **ffpu** has to be created.

**For possible endpoints read: [api endpoints](/docs/api.md)**

ffpapi can also serve the browser based frontend, just run in your browser `127.0.0.1:8787`.
