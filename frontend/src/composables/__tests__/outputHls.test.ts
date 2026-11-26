import assert from 'node:assert/strict'
import { test } from 'node:test'
import { selectHlsPackager } from '../outputHls.ts'

test('selecting native enables date/time mapping without changing the original options', () => {
    const options = { hls_delete_threshold: '2' }

    assert.deepEqual(selectHlsPackager(options, 'native'), {
        ...options,
        ffplayout_packager: 'native',
        hls_flags: 'program_date_time',
    })
    assert.deepEqual(options, { hls_delete_threshold: '2' })
    assert.deepEqual(selectHlsPackager(options, 'ffmpeg'), { ...options, ffplayout_packager: 'ffmpeg' })
})

test('existing native settings and disabled date/time mapping are preserved', () => {
    const options = { ffplayout_packager: 'native' }

    assert.deepEqual(selectHlsPackager(options, 'native'), options)
})

test('explicit muxer flags are preserved when changing packagers', () => {
    const options = { hls_flags: 'program_date_time+temp_file' }

    assert.equal(selectHlsPackager(options, 'native').hls_flags, options.hls_flags)
    assert.deepEqual(selectHlsPackager({ ...options, ffplayout_packager: 'native' }, 'ffmpeg'), {
        ...options,
        ffplayout_packager: 'ffmpeg',
    })
})

test('switching to FFmpeg removes the native segment container option', () => {
    assert.deepEqual(selectHlsPackager({ ffplayout_packager: 'native', hls_segment_type: 'fmp4' }, 'ffmpeg'), {
        ffplayout_packager: 'ffmpeg',
    })
})

test('switching to FFmpeg removes the native shared audio option', () => {
    assert.deepEqual(
        selectHlsPackager({ ffplayout_packager: 'native', hls_shared_audio: '1' }, 'ffmpeg'),
        { ffplayout_packager: 'ffmpeg' },
    )
})

test('leaving HLS removes the saved packager option', () => {
    assert.deepEqual(selectHlsPackager({ ffplayout_packager: 'ffmpeg' }), {})
})
