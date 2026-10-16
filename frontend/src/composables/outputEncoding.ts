type EncodingOutput = PlayoutConfigExt['output']

/** A derived HLS output publishes one rendition under its own playlist name. */
export function outputVariantNames(output: EncodingOutput): string[] {
    if (output.mode !== 'hls') return []
    return [
        output.hls_playlist_name,
        ...(output.encoding_source_id === null ? output.hls_variants.map((spec) => spec.split(':')[0]) : []),
    ]
}

/** Resolve the owner without modifying draft settings; invalid chains aren't offered as sources. */
export function encodingOwner(
    candidate: EncodingOutput,
    outputs: EncodingOutput[],
    destination?: EncodingOutput,
): EncodingOutput | undefined {
    const visited = new Set<EncodingOutput>()
    let current: EncodingOutput | undefined = candidate
    while (current) {
        if (current === destination || visited.has(current) || !current.enabled || current.mode === 'desktop') {
            return undefined
        }
        visited.add(current)
        if (current.encoding_source_id === null) return current
        const sourceId: number = current.encoding_source_id
        current = outputs.find((item) => item.id === sourceId)
    }
    return undefined
}
