export type LogoSizeMode = 'original' | 'pixels' | 'percent' | 'custom'
export type LogoSize = { mode: LogoSizeMode; width: number | ''; height: number | '' }

/** Preserve mixed units and legacy expressions in the advanced editor. */
export function parseLogoSize(value: string): LogoSize {
    if (!value.trim()) return { mode: 'original', width: '', height: '' }
    const parts = value.trim().split(/[:x]/)
    if (parts.length !== 2) return { mode: 'custom', width: '', height: '' }
    const dimensions = parts.map((part) => part.trim())
    const fixed = dimensions.filter((part) => part !== '-1')
    const mode = fixed.some((part) => part.endsWith('%')) ? 'percent' : 'pixels'
    const pattern = mode === 'percent' ? /^\d+(?:\.\d+)?%$/ : /^\d+$/
    if (fixed.some((part) => !pattern.test(part) || !Number.isFinite(Number(part.replace('%', ''))))) {
        return { mode: 'custom', width: '', height: '' }
    }
    const [width, height] = dimensions.map((part) => (part === '-1' ? '' : Number(part.replace('%', ''))))
    return { mode, width, height }
}

export function formatLogoSize(size: LogoSize): string {
    if (size.mode === 'original') return ''
    const unit = size.mode === 'percent' ? '%' : ''
    return [size.width, size.height].map((value) => (value === '' ? '-1' : `${value}${unit}`)).join(':')
}
