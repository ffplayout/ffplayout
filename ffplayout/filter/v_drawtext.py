import os
import re

from ffplayout.utils import _text


def filter(probe):
    font = ''
    source = os.path.basename(probe.src)
    match = re.match(_text.regex, source)
    title = match[1] if match else source

    if _text.fontfile and os.path.isfile(_text.fontfile):
        font = f":fontfile='{_text.fontfile}'"

    return f"drawtext=text='{title}':{_text.style}{font}"
