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

Vue.filter('filename', function (path) {
    if (path) {
        const pathArr = path.split('/')
        return pathArr[pathArr.length - 1]
    } else {
        return ''
    }
})
