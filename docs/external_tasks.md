# External Tasks

An external task runs a script or binary when a non-skipped clip starts. Enable
it and set the executable path in the channel playout settings.

ffplayout passes the current playout data as a JSON object in the first
command-line argument. It includes the current media, playout mode, elapsed
time, time shift, live-ingest state, and available audio-level data.

For example, a shell script can read the JSON argument as its first parameter:

```sh
#!/bin/sh
data="$1"
printf '%s\n' "$data" >> /var/log/ffplayout-task.log
```

The configured file must be executable by the user running ffplayout.

## Lifecycle Limits

Only one external task can run per channel. Starting the next clip task first
terminates a still-running task from the previous clip. Stopping the channel
also terminates its task. A task that runs longer than 30 seconds is terminated
automatically.

External tasks should finish quickly. Use a separate service, queue, or job
runner for work that needs to continue beyond the clip boundary.
