import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc'
import timezone from 'dayjs/plugin/timezone'
dayjs.extend(utc)
dayjs.extend(timezone)

import { useConfig } from '@/stores/config'

export const stringFormatter = () => {
    function fileSize(bytes: number | undefined, dp = 2) {
        if (!bytes) {
            return 0.0
        }

        const thresh = 1024

        if (Math.abs(bytes) < thresh) {
            return bytes + ' B'
        }

        const units = ['KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB', 'ZiB', 'YiB']
        let u = -1
        const r = 10 ** dp

        do {
            bytes /= thresh
            ++u
        } while (Math.round(Math.abs(bytes) * r) / r >= thresh && u < units.length - 1)

        return bytes.toFixed(dp) + ' ' + units[u]
    }

    function timeToSeconds(time: string): number {
        const t = time.split(':')
        return parseInt(t[0]) * 3600 + parseInt(t[1]) * 60 + parseInt(t[2])
    }

    function secToHMS(sec: number): string {
        const sign = Math.sign(sec)
        sec = Math.abs(sec)

        const hours = Math.floor(sec / 3600)
        sec %= 3600
        const minutes = Math.floor(sec / 60)
        const seconds = Math.round(sec % 60)

        const m = String(minutes).padStart(2, '0')
        const h = String(hours).padStart(2, '0')
        const s = String(seconds).padStart(2, '0')

        const hString = (sign === -1 ? '-' : '') + h

        return `${hString}:${m}:${s}`
    }

    function numberToHex(num: number): string {
        return '0x' + Math.round(num * 255).toString(16)
    }

    function hexToNumber(num: string): number {
        return parseFloat((parseFloat(parseInt(num, 16).toString()) / 255).toFixed(2))
    }

    function filename(path: string): string {
        if (path) {
            const pathArr = path.split('/')
            const name = pathArr[pathArr.length - 1]

            if (name) {
                return name
            } else {
                return path
            }
        } else {
            return ''
        }
    }

    function parent(path: string): string {
        if (path) {
            const pathArr = path.split('/')
            pathArr.pop()

            if (pathArr.length > 0) {
                return pathArr.join('/')
            } else {
                return '/'
            }
        } else {
            return ''
        }
    }

    function toMin(sec: number): string {
        if (sec) {
            const minutes = Math.floor(sec / 60)
            const seconds = Math.round(sec - minutes * 60)
            return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')} min`
        } else {
            return ''
        }
    }

    function secondsToTime(sec: number) {
        return new Date(sec * 1000 || 0).toISOString().substring(11, 19)
    }

    function mediaType(path: string) {
        const liveType = ['m3u8']
        const videoType = [
            'avi',
            'flv',
            'm2v',
            'm4v',
            'mkv',
            'mov',
            'mp4',
            'mpeg',
            'mpg',
            'mts',
            'mxf',
            'ts',
            'vob',
            'ogv',
            'webm',
            'wmv',
        ]
        const audioType = ['aac', 'aiff', 'flac', 'm4a', 'mp2', 'mp3', 'ogg', 'opus', 'wav', 'wma']
        const imageType = [
            'apng',
            'avif',
            'bmp',
            'exr',
            'gif',
            'jpeg',
            'jpg',
            'png',
            'psd',
            'tga',
            'tif',
            'tiff',
            'webp',
        ]
        const ext = path.split('.').pop()

        if (ext) {
            if (liveType.includes(ext.toLowerCase())) {
                return 'live'
            } else if (videoType.includes(ext.toLowerCase())) {
                return 'video'
            } else if (audioType.includes(ext.toLowerCase())) {
                return 'audio'
            } else if (imageType.includes(ext.toLowerCase())) {
                return 'image'
            }
        }

        return null
    }

    function dir_file(path: string): { dir: string; file: string } {
        const index = path.lastIndexOf('/')
        const dir = path.substring(0, index + 1) || '/'
        const file = path.substring(index + 1)

        return { dir, file }
    }

    return {
        fileSize,
        timeToSeconds,
        secToHMS,
        numberToHex,
        hexToNumber,
        filename,
        parent,
        toMin,
        secondsToTime,
        mediaType,
        dir_file,
    }
}

export const playlistOperations = () => {
    function genUID() {
        return String(Date.now().toString(32) + Math.random().toString(16)).replace(/\./g, '')
    }

    function processPlaylist(date: string, list: PlaylistItem[], forSave: boolean) {
        const configStore = useConfig()

        let begin = configStore.playout.playlist.startInSec

        const newList = []

        for (const item of list) {
            if (configStore.playout.playlist.startInSec === begin) {
                item.date = date
            }

            if (forSave) {
                delete item.date

                if (!item.audio) {
                    delete item.audio
                }

                if (!item.category) {
                    delete item.category
                }

                if (!item.custom_filter) {
                    delete item.custom_filter
                }

                if (!item.title) {
                    delete item.title
                }

                if (
                    begin + (item.out - item.in) >
                    configStore.playout.playlist.startInSec + configStore.playout.playlist.lengthInSec
                ) {
                    item.out =
                        configStore.playout.playlist.startInSec + configStore.playout.playlist.lengthInSec - begin
                }
            } else {
                if (!item.uid) {
                    item.uid = genUID()
                }

                if (
                    begin >= configStore.playout.playlist.startInSec + configStore.playout.playlist.lengthInSec &&
                    !configStore.playout.playlist.infinit
                ) {
                    item.overtime = true
                } else if (item.overtime) {
                    delete item.overtime
                }

                item.begin = begin
            }

            newList.push(item)

            begin += item.out - item.in
        }

        return newList
    }

    return { processPlaylist, genUID }
}
