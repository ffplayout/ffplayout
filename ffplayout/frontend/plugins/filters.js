import Vue from 'vue'

Vue.filter('toMin', function (sec) {
    if (sec) {
        const minutes = Math.floor(sec / 60)
        const seconds = Math.round(sec - minutes * 60)
        return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')} min`
    } else {
        return ''
    }
})
