"""
custom video filter, which get loaded automatically
"""

import re
from pathlib import Path

from ..utils import lower_third


def filter_link(node):
    """
    extract title from file name and overlay it
    """
    font = ''
    source = str(Path(node.get('source')).name)
    match = re.match(lower_third.regex, source)
    title = match[1] if match else source

    if lower_third.fontfile and Path(lower_third.fontfile).is_file():
        font = f":fontfile='{lower_third.fontfile}'"

    if lower_third.text_from_filename:
        escape = title.replace("'", "'\\\\\\''").replace("%", "\\\\\\%")
        return f"drawtext=text='{escape}':{lower_third.style}{font}"

    return None
