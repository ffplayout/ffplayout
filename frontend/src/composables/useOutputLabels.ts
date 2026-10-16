import { useI18n } from 'vue-i18n'

const settingLabels: Record<string, string> = {
    Preset: 'preset',
    Tune: 'tune',
    'Rate control': 'rateControl',
    Quality: 'quality',
    'Constant quality': 'constantQuality',
    'Global quality': 'globalQuality',
    'Constant QP': 'constantQp',
    'Maximum bitrate (kbit/s)': 'maxrate',
    'Encoding mode': 'mode',
    Speed: 'speed',
    'Row multithreading': 'rowMultithreading',
}

const choiceLabels: Record<string, string> = {
    'Good quality': 'goodQuality',
    Realtime: 'realtime',
    Automatic: 'automatic',
    Enabled: 'enabled',
    Disabled: 'disabled',
    'P1 (fastest)': 'p1Fastest',
    'P4 (default)': 'p4Default',
    'P7 (best quality)': 'p7BestQuality',
    '0 (slowest)': 'slowest',
    '8 (default)': 'defaultSpeed',
    '13 (fastest)': 'fastest',
}

/** Translate display text while retaining the encoder's actual option values. */
export function useOutputLabels() {
    const { t, te } = useI18n()

    function outputModeLabel(mode: string) {
        const key = `config.outputModes.${mode}`
        return te(key) ? t(key) : mode
    }

    function outputStatusLabel(status?: string | null) {
        const key = `config.outputStates.${status || 'unknown'}`
        return te(key) ? t(key) : t('config.outputStates.unknown')
    }

    function encoderSettingLabel(setting: EncoderSetting) {
        const key = settingLabels[setting.label]
        return key ? t(`config.encoderLabels.${key}`) : setting.label
    }

    function encoderChoiceLabel(choice: EncoderSettingChoice) {
        const key = choiceLabels[choice.label]
        return key ? t(`config.encoderChoices.${key}`) : choice.label
    }

    return { outputModeLabel, outputStatusLabel, encoderSettingLabel, encoderChoiceLabel }
}
