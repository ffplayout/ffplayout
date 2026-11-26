<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { useI18n } from 'vue-i18n'
import type { GraphicAppointment, ManualGraphicLayer } from '@/composables/manualGraphics'

const props = defineProps<{ layer: ManualGraphicLayer; timezone?: string; busy: boolean }>()
const emit = defineEmits<{ command: [data: object] }>()
const { locale, t } = useI18n()
const timezone = computed(() => props.timezone ?? 'UTC')

type AppointmentDraft = { id: number; weekly: boolean; byEndTime: boolean; appointment: GraphicAppointment }

function normalizeTime(time: string) {
    return time.length === 5 ? time + ':00' : time
}

function serialize(appointments: GraphicAppointment[]) {
    return JSON.stringify(
        appointments.map((appointment) => ({
            enabled: appointment.enabled,
            date: appointment.date,
            weekdays: [...appointment.weekdays].sort(),
            start_date: appointment.start_date,
            end_date: appointment.end_date,
            start_time: normalizeTime(appointment.start_time),
            end_time: appointment.end_time ? normalizeTime(appointment.end_time) : null,
            duration_seconds: appointment.duration_seconds,
        })),
    )
}

let nextId = 0
const drafts = ref<AppointmentDraft[]>([])
const baseline = ref<GraphicAppointment[]>([])
const stored = computed(() => serialize(props.layer.appointments ?? []))
const appointments = computed<GraphicAppointment[]>(() =>
    drafts.value.map(({ weekly, byEndTime, appointment }) => ({
        ...appointment,
        date: weekly ? null : appointment.date || null,
        weekdays: weekly ? appointment.weekdays : [],
        start_date: weekly ? appointment.start_date || null : null,
        end_date: weekly ? appointment.end_date || null : null,
        end_time: byEndTime ? appointment.end_time || null : null,
        duration_seconds: byEndTime ? null : appointment.duration_seconds,
    })),
)
const draftValue = computed(() => serialize(appointments.value))
const baselineValue = computed(() => serialize(baseline.value))
const dirty = computed(() => draftValue.value !== baselineValue.value)
const conflict = computed(
    () => dirty.value && stored.value !== baselineValue.value && stored.value !== draftValue.value,
)
const valid = computed(() =>
    appointments.value.every((appointment, index) => {
        const draft = drafts.value[index]!
        return (
            !!appointment.start_time &&
            (draft.byEndTime
                ? !!appointment.end_time &&
                  normalizeTime(appointment.end_time) !== normalizeTime(appointment.start_time)
                : Number.isInteger(appointment.duration_seconds) &&
                  appointment.duration_seconds! > 0 &&
                  appointment.duration_seconds! <= 4_294_967_295) &&
            (draft.weekly
                ? appointment.weekdays.length > 0 &&
                  (!appointment.start_date || !appointment.end_date || appointment.start_date <= appointment.end_date)
                : !!appointment.date)
        )
    }),
)
const weekdays = computed(() => {
    const formatter = new Intl.DateTimeFormat(locale.value, { weekday: 'short', timeZone: 'UTC' })
    return Array.from({ length: 7 }, (_, day) => formatter.format(new Date(Date.UTC(2024, 0, day + 1))))
})

function reset() {
    baseline.value = cloneDeep(props.layer.appointments ?? [])
    drafts.value = (props.layer.appointments ?? []).map((appointment) => ({
        id: nextId++,
        weekly: appointment.date === null,
        byEndTime: appointment.end_time !== null,
        appointment: cloneDeep(appointment),
    }))
}

watch(
    stored,
    (current) => {
        if (!dirty.value || current === draftValue.value) reset()
    },
    { immediate: true },
)

function save() {
    if (!dirty.value || conflict.value || !valid.value) return
    emit('command', {
        action: 'schedule',
        layer: props.layer.layer,
        appointments: appointments.value,
        expected_appointments: baseline.value,
    })
}

function add() {
    drafts.value.push({
        id: nextId++,
        weekly: false,
        byEndTime: false,
        appointment: {
            enabled: true,
            date: null,
            weekdays: [],
            start_date: null,
            end_date: null,
            start_time: '12:00:00',
            end_time: null,
            duration_seconds: 30,
        },
    })
}

function formatTimestamp(timestamp: string) {
    return new Intl.DateTimeFormat(locale.value, {
        dateStyle: 'medium',
        timeStyle: 'medium',
        timeZone: timezone.value,
    }).format(new Date(timestamp))
}
</script>

<template>
    <details class="collapse collapse-arrow mt-4 border border-base-content/20">
        <summary class="collapse-title text-sm font-medium">
            {{ t('graphicsSchedule.title') }} <span class="text-base-content/70">· {{ timezone }}</span>
            <span v-if="dirty" class="ml-2 text-xs text-warning">{{ t('graphicsPage.unsaved') }}</span>
        </summary>
        <div class="collapse-content">
            <p v-if="conflict" role="alert" class="mb-3 text-sm text-warning">
                {{ t('graphicsSchedule.conflict') }}
            </p>
            <div v-if="layer.schedule_active_until || layer.schedule_next_start" class="mb-3 text-sm">
                <p v-if="layer.schedule_active_until">
                    {{ t('graphicsSchedule.activeUntil', { time: formatTimestamp(layer.schedule_active_until) }) }}
                </p>
                <p v-if="layer.schedule_next_start">
                    {{ t('graphicsSchedule.nextStart', { time: formatTimestamp(layer.schedule_next_start) }) }}
                </p>
            </div>
            <fieldset :disabled="busy" class="min-w-0 space-y-3">
                <div
                    v-for="(draft, index) in drafts"
                    :key="draft.id"
                    class="rounded-box border border-base-content/20 p-3"
                >
                    <div class="flex items-center justify-between gap-2">
                        <label class="label cursor-pointer">
                            <input v-model="draft.appointment.enabled" type="checkbox" class="checkbox checkbox-sm" />
                            {{ t('graphicsSchedule.enabled') }}
                        </label>
                        <button class="btn btn-xs btn-ghost text-error" type="button" @click="drafts.splice(index, 1)">
                            {{ t('graphicsSchedule.remove') }}
                        </button>
                    </div>
                    <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                        <label class="fieldset min-w-0">
                            <span>{{ t('graphicsSchedule.recurrence') }}</span>
                            <select v-model="draft.weekly" class="select select-sm w-full">
                                <option :value="false">{{ t('graphicsSchedule.once') }}</option>
                                <option :value="true">{{ t('graphicsSchedule.weekly') }}</option>
                            </select>
                        </label>
                        <label v-if="!draft.weekly" class="fieldset min-w-0">
                            <span>{{ t('graphicsSchedule.date') }}</span>
                            <input v-model="draft.appointment.date" type="date" class="input input-sm w-full" />
                        </label>
                        <template v-else>
                            <label class="fieldset min-w-0">
                                <span>{{ t('graphicsSchedule.from') }}</span>
                                <input
                                    v-model="draft.appointment.start_date"
                                    type="date"
                                    class="input input-sm w-full"
                                />
                            </label>
                            <label class="fieldset min-w-0">
                                <span>{{ t('graphicsSchedule.through') }}</span>
                                <input v-model="draft.appointment.end_date" type="date" class="input input-sm w-full" />
                            </label>
                            <fieldset class="fieldset sm:col-span-2 lg:col-span-3">
                                <legend class="fieldset-legend">{{ t('graphicsSchedule.weekdays') }}</legend>
                                <div class="flex flex-wrap gap-x-3 gap-y-2">
                                    <label
                                        v-for="(day, weekday) in weekdays"
                                        :key="weekday"
                                        class="label cursor-pointer"
                                    >
                                        <input
                                            v-model="draft.appointment.weekdays"
                                            type="checkbox"
                                            :value="weekday + 1"
                                            class="checkbox checkbox-sm"
                                        />
                                        {{ day }}
                                    </label>
                                </div>
                            </fieldset>
                        </template>
                        <label class="fieldset min-w-0">
                            <span>{{ t('graphicsSchedule.start') }}</span>
                            <input
                                v-model="draft.appointment.start_time"
                                type="time"
                                step="1"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset min-w-0">
                            <span>{{ t('graphicsSchedule.endBy') }}</span>
                            <select v-model="draft.byEndTime" class="select select-sm w-full">
                                <option :value="false">{{ t('graphicsSchedule.duration') }}</option>
                                <option :value="true">{{ t('graphicsSchedule.end') }}</option>
                            </select>
                        </label>
                        <label v-if="draft.byEndTime" class="fieldset min-w-0">
                            <span>{{ t('graphicsSchedule.end') }}</span>
                            <input
                                v-model="draft.appointment.end_time"
                                type="time"
                                step="1"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label v-else class="fieldset min-w-0">
                            <span>{{ t('graphicsSchedule.duration') }}</span>
                            <input
                                v-model.number="draft.appointment.duration_seconds"
                                type="number"
                                min="1"
                                max="4294967295"
                                step="1"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                </div>
                <div class="flex flex-wrap gap-2">
                    <button type="button" class="btn btn-sm" @click="add">{{ t('graphicsSchedule.add') }}</button>
                    <button
                        type="button"
                        class="btn btn-sm btn-primary"
                        :disabled="!dirty || !valid || conflict"
                        @click="save"
                    >
                        {{ t('graphicsSchedule.save') }}
                    </button>
                    <button v-if="dirty" type="button" class="btn btn-sm btn-ghost" @click="reset">
                        {{ t('graphicsPage.resetEdits') }}
                    </button>
                </div>
            </fieldset>
        </div>
    </details>
</template>
