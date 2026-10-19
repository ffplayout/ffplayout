export interface MoqBootstrap {
    broadcast: string
    relay_url: string | null
    relay_port: number
    delay_ms: number
    ticket: string | null
}

/** Use the same relay and development proxy for playback and meter subscriptions. */
export function moqRelayUrl(config: MoqBootstrap, channel: number) {
    const relay = new URL('/moq/relay', location.href)
    relay.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
    if (!import.meta.env.DEV) {
        if (config.relay_url) relay.href = config.relay_url
        else if (location.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(location.hostname)) {
            relay.href = `http://${location.hostname}:${config.relay_port}/channels`
        }
    }
    relay.searchParams.set('channel', String(channel))
    if (config.ticket) relay.searchParams.set('ticket', config.ticket)
    return relay
}
