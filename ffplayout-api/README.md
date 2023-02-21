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

"Piggyback" Mode
-----

ffplayout was originally planned to run under Linux as a SystemD service. It is also designed so that the engine and ffpapi run completely independently of each other. This is to increase flexibility and stability.

Nevertheless, programs compiled in Rust can basically run on all systems supported by the language. And so this repo also offers binaries for other platforms.

In the past, however, it was only possible under Linux to start/stop/restart the ffplayout engine process through ffpapi. This limit no longer exists since v0.17.0, because the "piggyback" mode was introduced here. This means that ffpapi recognizes which platform it is running on, and if it is not on Linux, it starts the engine as a child process. Thus it is now possible to control ffplayout engine completely on all platforms. The disadvantage here is that the engine process is dependent on ffpapi; if it closes or crashes, the engine also closes.

Under Linux, this mode can be simulated by starting ffpapi with the environment variable `PIGGYBACK_MODE=true`. This scenario is also conceivable in container operation, for example.

**Run in piggyback mode:**

```BASH
PIGGYBACK_MODE=True ffpapi -l 127.0.0.1:8787
```

This function is experimental, use it with caution.
