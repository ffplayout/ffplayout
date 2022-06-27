**ffplayout-api**
================

ffplayout-api (ffpapi) is a non strict REST API for ffplayout. It makes it possible to control the engine, read and manipulate the config, save playlist, etc.

To be able to use the API it is necessary to initialize the settings database first. To do that, run:

```BASH
ffpapi -i
```

Then add an admin user:

```BASH
ffpapi -u <USERNAME> -p <PASSWORD> -e <EMAIL ADDRESS>
```

Then run the API thru the systemd service, or like:

```BASH
ffpapi -l 127.0.0.1:8080
```

If you plan to run ffpapi with systemd set permission from **/usr/share/ffplayout** and content to user **www-data:www-data**.

**For possible endpoints read: [api endpoints](/docs/api.md)**
