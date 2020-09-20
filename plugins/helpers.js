export default ({ app }, inject) => {
    inject('processPlaylist', (dayStart, list) => {
        if (!dayStart) {
            dayStart = '00:00:00'
        }
        const [h, m, s] = dayStart.split(':')
        let begin = parseFloat(h) * 3600 + parseFloat(m) * 60 + parseFloat(s)

        for (const item of list) {
            item.begin = begin

            if (!item.category) {
                item.category = ''
            }

            begin += (item.out - item.in)
        }
        return list
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
