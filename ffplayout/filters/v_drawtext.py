"""
cunstom video filter, which get loaded automatically
"""

import os
import re

from ffplayout.utils import lower_third


def filter_link(node):
    """
    extract title from file name and overlay it
    """
    font = ''
    source = os.path.basename(node.get('source'))
    match = re.match(lower_third.regex, source)
    title = match[1] if match else source

    if lower_third.fontfile and os.path.isfile(lower_third.fontfile):
        font = f":fontfile='{lower_third.fontfile}'"

    if lower_third.text_from_filename:
        escape = title.replace("'", "'\\\\\\''").replace("%", "\\\\\\%")
        return f"drawtext=text='{escape}':{lower_third.style}{font}"

    return None
