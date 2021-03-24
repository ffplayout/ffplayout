import os
import re

from ffplayout.utils import TEXT


def filter_link(node):
    """
    extract title from file name and overlay it
    """
    font = ''
    source = os.path.basename(node.get('source'))
    match = re.match(TEXT.regex, source)
    title = match[1] if match else source

    if TEXT.fontfile and os.path.isfile(TEXT.fontfile):
        font = f":fontfile='{TEXT.fontfile}'"

    if TEXT.text_from_filename:
        escape = title.replace("'", "'\\\\\\''").replace("%", "\\\\\\%")
        return f"drawtext=text='{escape}':{TEXT.style}{font}"
