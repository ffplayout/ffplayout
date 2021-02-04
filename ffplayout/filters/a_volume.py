from ffplayout.utils import get_float, stdin_args


def filter(probe, node=None):
    """
    set audio volume
    """

    if stdin_args.volume and get_float(stdin_args.volume, False):
        return f'volume={stdin_args.volume}'
