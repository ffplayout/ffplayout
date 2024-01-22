#!/usr/bin/env python3

import json
import sys

# install plyer: pip install plyer
from plyer import notification


def send_notification(title, message):
    notification.notify(
        title=title,
        message=message,
        timeout=10
    )


if __name__ == "__main__":
    title = "ffplayout - current clip:"
    input_data = json.loads(sys.argv[1]).get('current_media')

    if input_data is not None:
        # print(input_data['source'])

        send_notification(title, f"Play: \"{input_data['source']}\"")
