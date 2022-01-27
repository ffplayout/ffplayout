#!/usr/bin/env python3

# This file is part of ffplayout.
#
# ffplayout is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# ffplayout is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with ffplayout. If not, see <http://www.gnu.org/licenses/>.

# ------------------------------------------------------------------------------

"""
This module is the starting program for running ffplayout engine.
"""

from importlib import import_module
from pathlib import Path
from platform import system

from ffplayout.utils import playout, messenger, validate_ffmpeg_libs

try:
    if system() == 'Windows':
        import colorama
        colorama.init()
except ImportError:
    print('colorama import failed, no colored console output on windows...')


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

def main():
    """
    play out depending on output mode
    """

    script_dir = Path(__file__).parent.absolute()
    output_dir = script_dir.joinpath('ffplayout', 'output')

    for output in output_dir.glob('*.py'):
        if output != '__init__.py':
            mode = Path(output).stem

            if mode == playout.mode:
                output = import_module(f'ffplayout.output.{mode}').output
                output()
    else:
        messenger.error('Output mode not exist!')


if __name__ == '__main__':
    # check if ffmpeg contains all codecs and filters
    validate_ffmpeg_libs()
    main()
