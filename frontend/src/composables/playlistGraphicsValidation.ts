import { computed, ref } from 'vue'
import { readGraphicEvents, validateGraphicEvents } from './playlistGraphics.ts'

/** One clip-dialog session: preserve initially invalid data until graphics are edited. */
export function usePlaylistGraphicsValidation(value: () => unknown, duration: () => number) {
    const modified = ref(false)
    const events = computed(() => readGraphicEvents(value()))
    const issues = computed(() => (events.value ? validateGraphicEvents(events.value, duration()) : []))
    const initiallyInvalid = events.value === undefined || issues.value.length > 0
    // In/Out changes can expose previously ignored actions. An initially valid
    // timeline must stay valid even when the graphic fields themselves are untouched.
    const canSave = computed(
        () => (!modified.value && initiallyInvalid) || (events.value !== undefined && issues.value.length === 0),
    )

    return { modified, events, issues, canSave }
}
