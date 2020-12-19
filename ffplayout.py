#!/usr/bin/env python3
# -*- coding: utf-8 -*-

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

import os
from pydoc import locate

from ffplayout.utils import _playout, stdin_args, validate_ffmpeg_libs

try:
    if os.name != 'posix':
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

    if stdin_args.mode:
        output = locate('ffplayout.output.{}.output'.format(stdin_args.mode))
        output()

    else:
        script_dir = os.path.dirname(os.path.abspath(__file__))
        output_dir = os.path.join(script_dir, 'ffplayout', 'output')

        for output in os.listdir(output_dir):
            if os.path.isfile(os.path.join(output_dir, output)) \
                    and output != '__init__.py':
                mode = os.path.splitext(output)[0]

                if mode == _playout.mode:
                    output = locate('ffplayout.output.{}.output'.format(mode))
                    output()


if __name__ == '__main__':
    # check if ffmpeg contains all codecs and filters
    validate_ffmpeg_libs()
    main()
