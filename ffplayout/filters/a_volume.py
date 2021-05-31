"""
custom audio filter, which get loaded automatically
"""

from ..utils import get_float, stdin_args


# pylint: disable=unused-argument
def filter_link(node):
    """
    set audio volume
    """

    if stdin_args.volume and get_float(stdin_args.volume, False):
        return f'volume={stdin_args.volume}'

    return None
