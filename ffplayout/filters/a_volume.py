from ffplayout.utils import STDIN_ARGS, get_float


def filter_link(node):
    """
    set audio volume
    """

    if STDIN_ARGS.volume and get_float(STDIN_ARGS.volume, False):
        return f'volume={STDIN_ARGS.volume}'
