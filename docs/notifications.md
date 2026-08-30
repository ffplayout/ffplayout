# HTTP Push Notifications

ffplayout can send rate-limited log alerts to a topic-based HTTP push service.
The publish request format is compatible with [ntfy](https://docs.ntfy.sh/publish/),
including self-hosted ntfy servers. It uses the service's HTTP interface directly;
no additional notification-specific dependency is required.

## Configuration

Configure the shared connection under **Configuration → Global → HTTP Push
Notifications**. This requires a global admin.

- **Push Server**: The base HTTP or HTTPS URL, for example `https://ntfy.sh` or
  `https://push.example.org`.
- **Access Token**: Optional bearer token used to authenticate with the push
  service. Leaving the field empty after a token has been saved keeps the
  existing token.

After saving a push server, configure each channel under **Configuration →
Playout → HTTP Push Notifications**:

- **Topic**: The channel/topic path at the push service, for example
  `ffplayout-alerts`.
- **Minimum Level**: The lowest log severity that creates a notification.
- **Tags**: Optional comma-separated tags passed to services that support them,
  such as ntfy.

The channel-specific notification section is visible only after a push server
has been configured globally. Topic, level, and tags can be changed without
restarting playout.

## Request format

For a server URL of `https://ntfy.sh` and topic `ffplayout-alerts`, ffplayout
sends a `POST` request to:

```text
https://ntfy.sh/ffplayout-alerts
```

The log message is sent as plain-text request body. Requests contain these
headers:

- `Title`: Generated as `ffplayout channel <id> <level>`.
- `Priority`: `5` for fatal errors, `4` for errors, `3` for warnings, and `2`
  for informational messages.
- `Tags`: Sent when configured for the channel.
- `Authorization: Bearer <token>`: Sent when an access token is configured.

`Topic` is the destination path, not the visible notification title. The title
is generated so that alerts remain identifiable when several channels use the
same topic.

## Levels and rate limiting

The available minimum levels are `FATAL`, `ERROR`, `WARNING`, and `INFO`.
Selecting `ERROR` sends both regular error records and fatal records.

To avoid alert floods, limits are applied independently per channel:

- The same non-fatal notification fingerprint is suppressed for five minutes.
- At most five non-fatal notifications are sent in a ten-minute window.
- A fatal notification is sent immediately, then further fatal notifications
  are limited to one every five minutes.

When a later notification is permitted, its body reports how many earlier
notifications were suppressed. For validation errors, playlist position and
scheduled time are ignored only for the notification fingerprint: normal log
files still retain the complete message, while the same broken source used at
multiple playlist positions produces one alert.

Errors raised while sending a notification are not sent as notifications
themselves, which prevents recursive alert loops. Check the ffplayout logs if a
push service rejects a request or cannot be reached. HTTP notification requests
time out after ten seconds so an unresponsive service cannot retain background
tasks indefinitely.
