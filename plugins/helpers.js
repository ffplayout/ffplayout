export default ({ app }, inject) => {
    inject('processPlaylist', (dayStart, length, list, forSave) => {
        if (!dayStart) {
            dayStart = 0
        }

        let begin = dayStart
        const newList = []

        for (const item of list) {
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
                    item.out = (length + dayStart) - begin
                }
            }

            if (forSave && begin >= length + dayStart) {
                break
            }

            newList.push(item)

            begin += (item.out - item.in)
        }

        return newList
    })

    // convert time (00:00:00) string to seconds
    inject('timeToSeconds', (time) => {
        const t = time.split(':')
        return parseInt(t[0]) * 3600 + parseInt(t[1]) * 60 + parseInt(t[2])
    })

    inject('secToHMS', (sec) => {
        let hours = Math.floor(sec / 3600)
        sec %= 3600
        let minutes = Math.floor(sec / 60)
        let seconds = sec % 60

        minutes = String(minutes).padStart(2, '0')
        hours = String(hours).padStart(2, '0')
        seconds = String(parseInt(seconds)).padStart(2, '0')
        return hours + ':' + minutes + ':' + seconds
    })
}
