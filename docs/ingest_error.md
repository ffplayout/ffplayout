In some situations, application closure may occur in conjunction with Live Ingest.

Here is an example in combination with SRS:

When a live stream is sent, it is forwarded to ffplayout, which then switches the TV program to the live stream.

Problems can occur if the internet connection for the live stream is not stable. In such cases, timeouts can occur, SRS breaks the connection to the playout, and the entire ffplayout process has to be restarted. The default timeout is 5000ms, or 5 seconds.

The timeout can be changed in SRS in the respective vhosts with:

```NGINX
publish {
    normal_timeout 30000;
}
```

Here the new timeout would be 30 seconds.

The error behavior can be simulated under Linux using the tool **tc**. Then carry out the following steps:

- Start SRS
- start ffplayout, with rtmp target SRS
- after a few seconds start a livestream to ffplayout
- shortly afterwards start **tc**: `tc qdisc add dev eth0 root netem loss 70%`
- wait until the timeout time is exceeded
- ffplayout should close the process with encoder errors
- undo **tc** rule: `tc qdisc delete dev eth0 root`

`eth0` must be replaced with the physical network interface.

Reference:
- [simulate-delayed-and-dropped-packets-on-linux](https://stackoverflow.com/questions/614795/simulate-delayed-and-dropped-packets-on-linux)
- [publish-normal-timeout](https://ossrs.io/lts/en-us/docs/v4/doc/special-control/#publish-normal-timeout)
