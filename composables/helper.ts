export const stringFormatter = () => {
    function formatLog(text: string) {
        return (
            text
                .replace(/\x1B\[33m(.*?)\x1B\[0m/g, '<span class="log-number">$1</span>')
                .replace(/\x1B\[1m\x1B\[35m(.*?)\x1B\[0m\x1B\[22m/g, '<span class="log-addr">$1</span>')
                .replace(/\x1B\[94m(.*?)\x1B\[0m/g, '<span class="log-cmd">$1</span>')
                .replace(/\x1B\[90m(.*?)\x1B\[0m/g, '<span class="log-debug">$1</span>')
                .replace(/(\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}.[\d]+\])/g, '<span class="log-time">$1</span>')
                .replace(/\[ INFO\]/g, '<span class="log-info">[ INFO]</span>')
                .replace(/\[ WARN\]/g, '<span class="log-warning">[ WARN]</span>')
                .replace(/\[ERROR\]/g, '<span class="log-error">[ERROR]</span>')
                .replace(/\[DEBUG\]/g, '<span class="log-debug">[DEBUG]</span>')
                .replace(/\[Decoder\]/g, '<span class="log-decoder">[Decoder]</span>')
                .replace(/\[Encoder\]/g, '<span class="log-encoder">[Encoder]</span>')
                .replace(/\[Server\]/g, '<span class="log-server">[Server]</span>')
                .replace(/\[Validator\]/g, '<span class="log-server">[Validator]</span>')
        )
    }

    function timeToSeconds(time: string) {
        const t = time.split(':')
        return parseInt(t[0]) * 3600 + parseInt(t[1]) * 60 + parseInt(t[2])
    }

    function secToHMS(sec: number) {
        let hours = Math.floor(sec / 3600)
        sec %= 3600
        let minutes = Math.floor(sec / 60)
        let seconds = Math.round(sec % 60)

        const m = String(minutes).padStart(2, '0')
        const h = String(hours).padStart(2, '0')
        const s = String(seconds).padStart(2, '0')
        return `${h}:${m}:${s}`
    }

    function numberToHex(num: number) {
        return '0x' + Math.round(num * 255).toString(16)
    }

    function hexToNumber(num: string): number {
        return parseFloat((parseFloat(parseInt(num, 16).toString()) / 255).toFixed(2))
    }

    function filename(path: string) {
        if (path) {
            const pathArr = path.split('/')
            return pathArr[pathArr.length - 1]
        } else {
            return ''
        }
    }

    function toMin(sec: number) {
        if (sec) {
            const minutes = Math.floor(sec / 60)
            const seconds = Math.round(sec - minutes * 60)
            return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')} min`
        } else {
            return ''
        }
    }

    function secondsToTime(sec: number) {
        return new Date(sec * 1000).toISOString().substr(11, 8)
    }

    return { formatLog, timeToSeconds, secToHMS, numberToHex, hexToNumber, filename, toMin, secondsToTime }
}

export const playlistOperations = () => {
    function genUID() {
        return String(
            Date.now().toString(32) +
            Math.random().toString(16)
        ).replace(/\./g, '')
    }

    function processPlaylist(dayStart: number, length: number, list: PlaylistItem[], forSave: boolean) {
        if (!dayStart) {
            dayStart = 0
        }

        let begin = dayStart
        const newList = []

        for (const item of list) {
            if (!item.uid) {
                item.uid = genUID()
            }

            item.begin = begin

            if (!item.audio) {
                delete item.audio
            }

            if (!item.category) {
                delete item.category
            }

            if (!item.custom_filter) {
                delete item.custom_filter
            }

            if (begin + (item.out - item.in) > length + dayStart) {
                item.class = 'overLength'

                if (forSave) {
                    item.out = length + dayStart - begin
                }
            }

            if (forSave && begin >= length + dayStart) {
                break
            }

            newList.push(item)

            begin += item.out - item.in
        }

        return newList
    }

    return { processPlaylist, genUID }
}
