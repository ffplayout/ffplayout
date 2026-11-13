/** Apply defaults only when explicitly switching to the native packager. */
export function selectHlsPackager(options: Record<string, string>, packager: string): Record<string, string> {
    const updated = { ...options }
    const previous = options.ffplayout_packager ?? 'ffmpeg'

    if (packager === 'native') {
        updated.ffplayout_packager = packager
        if (previous !== 'native' && !Object.hasOwn(options, 'hls_flags')) {
            updated.hls_flags = 'program_date_time'
        }
    } else {
        delete updated.ffplayout_packager
    }

    return updated
}
