/** Apply native defaults on selection and clear the packager when leaving HLS. */
export function selectHlsPackager(options: Record<string, string>, packager?: string): Record<string, string> {
    const updated = { ...options }
    const previous = options.ffplayout_packager ?? 'ffmpeg'

    if (packager === 'native') {
        updated.ffplayout_packager = packager
        if (previous !== 'native' && !Object.hasOwn(options, 'hls_flags')) {
            updated.hls_flags = 'program_date_time'
        }
    } else {
        if (packager === 'ffmpeg') updated.ffplayout_packager = packager
        else delete updated.ffplayout_packager
        if (previous === 'native') {
            delete updated.hls_segment_type
            delete updated.hls_shared_audio
        }
    }

    return updated
}
