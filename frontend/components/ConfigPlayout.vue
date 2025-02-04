<template>
    <div class="max-w-[1200px] xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('config.playoutConf') }}</h2>
        <form
            v-if="configStore.playout"
            class="mt-10 grid md:grid-cols-[180px_auto] gap-5"
            @submit.prevent="onSubmitPlayout"
        >
            <div class="text-xl pt-3 md:text-right">{{ t('config.general') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.generalHelp') }}
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Stop Threshold</span>
                    </div>
                    <input
                        v-model="configStore.playout.general.stop_threshold"
                        type="number"
                        min="3"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.stopThreshold') }}</span>
                    </div>
                </label>
            </div>

            <template v-if="configStore.playout.mail.show">
                <div class="text-xl pt-3 md:text-right">{{ t('config.mail') }}:</div>
                <div class="md:pt-4">
                    <label class="form-control mb-2">
                        <div class="whitespace-pre-line">
                            {{ t('config.mailHelp') }}
                        </div>
                    </label>
                    <label class="form-control w-full mt-2">
                        <div class="label">
                            <span class="label-text !text-md font-bold">Subject</span>
                        </div>
                        <input
                            v-model="configStore.playout.mail.subject"
                            type="text"
                            name="subject"
                            class="input input-sm input-bordered w-full max-w-lg"
                        />
                    </label>
                    <label class="form-control w-full mt-2">
                        <div class="label">
                            <span class="label-text !text-md font-bold">Recipient</span>
                        </div>
                        <input
                            v-model="configStore.playout.mail.recipient"
                            type="text"
                            name="recipient"
                            class="input input-sm input-bordered w-full max-w-lg"
                        />
                    </label>
                    <label class="form-control w-full mt-2">
                        <div class="label">
                            <span class="label-text !text-md font-bold">Mail Level</span>
                        </div>
                        <select
                            v-model="configStore.playout.mail.mail_level"
                            class="select select-sm select-bordered w-full max-w-xs"
                        >
                            <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </label>
                    <label class="form-control w-full mt-2">
                        <div class="label">
                            <span class="label-text !text-md font-bold">Interval</span>
                        </div>
                        <input
                            v-model="configStore.playout.mail.interval"
                            type="number"
                            min="30"
                            step="10"
                            class="input input-sm input-bordered w-full max-w-36"
                        />
                        <div class="label">
                            <span class="text-sm select-text text-base-content/80">{{ t('config.mailInterval') }}</span>
                        </div>
                    </label>
                </div>
            </template>

            <div class="text-xl pt-3 md:text-right">{{ t('config.logging') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.logHelp') }}
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">ffmpeg Level</span>
                    </div>
                    <select
                        v-model="configStore.playout.logging.ffmpeg_level"
                        class="select select-sm select-bordered w-full max-w-xs"
                    >
                        <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                    </select>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Ingest Level</span>
                    </div>
                    <select
                        v-model="configStore.playout.logging.ingest_level"
                        class="select select-sm select-bordered w-full max-w-xs"
                    >
                        <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                    </select>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="flex flex-row">
                        <input
                            v-model="configStore.playout.logging.detect_silence"
                            type="checkbox"
                            class="checkbox checkbox-sm me-1 mt-2"
                        />
                        <div class="label">
                            <span class="label-text !text-md font-bold">Detect Silence</span>
                        </div>
                    </div>
                    <div class="label py-0">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.logDetect') }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Ignore Lines</span>
                    </div>
                    <input
                        v-model="formatIgnoreLines"
                        type="text"
                        class="input input-sm input-bordered w-full truncate"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.logIgnore') }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.processing') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.processingHelp') }}
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Mode</span>
                    </div>
                    <select
                        v-model="configStore.playout.processing.mode"
                        class="select select-sm select-bordered w-full max-w-xs"
                    >
                        <option v-for="mode in processingMode" :key="mode" :value="mode">{{ mode }}</option>
                    </select>
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.processing.audio_only"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Only</span>
                    </div>
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.processing.copy_audio"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Copy Audio</span>
                    </div>
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.processing.copy_video"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Copy Video</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Width</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.width"
                        type="number"
                        min="-1"
                        step="1"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Height</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.height"
                        type="number"
                        min="-1"
                        step="1"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Aspect</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.aspect"
                        type="number"
                        min="1"
                        step="0.001"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">FPS</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.fps"
                        type="number"
                        min="1"
                        step="0.01"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.processing.add_logo"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Add Logo</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.logo"
                        type="text"
                        name="logo"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingLogoPath')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Opacity</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.logo_opacity"
                        type="number"
                        min="0"
                        max="1"
                        step="0.01"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Scale</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.logo_scale"
                        type="text"
                        name="logo_scale"
                        class="input input-sm input-bordered w-full max-w-md"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingLogoScale')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Position</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.logo_position"
                        type="text"
                        name="logo_position"
                        class="input input-sm input-bordered w-full max-w-md"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingLogoPosition')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Tracks</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.audio_tracks"
                        type="number"
                        min="1"
                        max="255"
                        step="1"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingAudioTracks')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Track Index</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.audio_track_index"
                        type="number"
                        min="-1"
                        max="255"
                        step="1"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingAudioIndex')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Channels</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.audio_channels"
                        type="number"
                        min="1"
                        max="255"
                        step="1"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingAudioChannels')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Volumen</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.volume"
                        type="number"
                        min="0"
                        max="1"
                        step="0.001"
                        class="input input-sm input-bordered w-full max-w-36"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Custom Filter</span>
                    </div>
                    <textarea
                        v-model="configStore.playout.processing.custom_filter"
                        class="textarea textarea-bordered"
                        rows="3"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingCustomFilter')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full flex-row mt-0">
                    <input
                        v-model="configStore.playout.processing.override_filter"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Override custom Filter</span>
                    </div>
                </label>
                <div v-if="configStore.playout.processing.override_filter" class="label py-0">
                    <span class="text-sm select-text font-bold text-orange-500">{{
                        t('config.processingOverrideFilter')
                    }}</span>
                </div>
                <label class="form-control w-full mt-2">
                    <div class="flex flex-row">
                        <input
                            v-model="configStore.playout.processing.vtt_enable"
                            type="checkbox"
                            class="checkbox checkbox-sm me-1 mt-2"
                        />
                        <div class="label">
                            <span class="label-text !text-md font-bold">Enable VTT</span>
                        </div>
                    </div>
                    <div class="label py-0">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingVTTEnable')
                        }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">VTT Dummy</span>
                    </div>
                    <input
                        v-model="configStore.playout.processing.vtt_dummy"
                        type="text"
                        name="vtt_dummy"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.processingVTTDummy')
                        }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.ingest') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.ingestHelp') }}
                    </div>
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.ingest.enable"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Enable</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Input Param</span>
                    </div>
                    <input
                        v-model="configStore.playout.ingest.input_param"
                        type="text"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Custom Filter</span>
                    </div>
                    <textarea
                        v-model="configStore.playout.ingest.custom_filter"
                        class="textarea textarea-bordered"
                        rows="3"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{
                            t('config.ingestCustomFilter')
                        }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.playlist') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.playlistHelp') }}
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Day Start</span>
                    </div>
                    <input
                        v-model="configStore.playout.playlist.day_start"
                        type="text"
                        name="day_start"
                        class="input input-sm input-bordered w-full max-w-xs"
                        pattern="([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.playlistDayStart') }}</span>
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Length</span>
                    </div>
                    <input
                        v-model="configStore.playout.playlist.length"
                        type="text"
                        name="length"
                        class="input input-sm input-bordered w-full max-w-xs"
                        pattern="([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.playlistLength') }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="flex flex-row">
                        <input
                            v-model="configStore.playout.playlist.infinit"
                            type="checkbox"
                            class="checkbox checkbox-sm me-1 mt-2"
                        />
                        <div class="label">
                            <span class="label-text !text-md font-bold">Infinit</span>
                        </div>
                    </div>
                    <div class="label py-0">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.playlistInfinit') }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.storage') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.storageHelp') }}
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Filler</span>
                    </div>
                    <input
                        v-model="configStore.playout.storage.filler"
                        type="text"
                        name="filler"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.storageFiller') }}</span>
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Extensions</span>
                    </div>
                    <input
                        v-model="extensions"
                        type="text"
                        name="extensions"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.storageExtension') }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="flex flex-row">
                        <input
                            v-model="configStore.playout.storage.shuffle"
                            type="checkbox"
                            class="checkbox checkbox-sm me-1 mt-2"
                        />
                        <div class="label">
                            <span class="label-text !text-md font-bold">Shuffle</span>
                        </div>
                    </div>
                    <div class="label py-0">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.storageShuffle') }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.text') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.textHelp') }}
                    </div>
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.text.add_text"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Add Text</span>
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Font</span>
                    </div>
                    <input
                        v-model="configStore.playout.text.font"
                        type="text"
                        name="font"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.textFont') }}</span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="flex flex-row">
                        <input
                            v-model="configStore.playout.text.text_from_filename"
                            type="checkbox"
                            class="checkbox checkbox-sm me-1 mt-2"
                        />
                        <div class="label">
                            <span class="label-text !text-md font-bold">Text from File</span>
                        </div>
                    </div>
                    <div class="label py-0">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.textFromFile') }}</span>
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Style</span>
                    </div>
                    <input
                        v-model="configStore.playout.text.style"
                        type="text"
                        name="style"
                        class="input input-sm input-bordered w-full truncate"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.textStyle') }}</span>
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Regex</span>
                    </div>
                    <input
                        v-model="configStore.playout.text.regex"
                        type="text"
                        name="regex"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.textRegex') }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.task') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.taskHelp') }}
                    </div>
                </label>
                <label class="form-control w-full flex-row mt-2">
                    <input
                        v-model="configStore.playout.task.enable"
                        type="checkbox"
                        class="checkbox checkbox-sm me-1 mt-2"
                    />
                    <div class="label">
                        <span class="label-text !text-md font-bold">Enable</span>
                    </div>
                </label>
                <label class="form-control w-full">
                    <div class="label">
                        <span class="label-text text-base font-bold">Path</span>
                    </div>
                    <input
                        v-model="configStore.playout.task.path"
                        type="text"
                        name="task_path"
                        class="input input-sm input-bordered w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.taskPath') }}</span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.output') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.outputHelp') }}
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Mode</span>
                    </div>
                    <select
                        v-model="configStore.playout.output.mode"
                        class="select select-sm select-bordered w-full max-w-xs"
                    >
                        <option v-for="mode in outputMode" :key="mode" :value="mode">{{ mode }}</option>
                    </select>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Output Parameter</span>
                    </div>
                    <textarea
                        v-model="configStore.playout.output.output_param"
                        class="textarea textarea-bordered"
                        rows="6"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">
                            {{ t('config.outputParam') }}
                        </span>
                    </div>
                </label>
            </div>
            <div class="mt-5 mb-10">
                <button class="btn btn-primary" type="submit">{{ t('config.save') }}</button>
            </div>
        </form>
    </div>

    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="configStore.showRestartModal"
        :modal-action="configStore.restart"
    />
</template>

<script setup lang="ts">
const { t } = useI18n()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const logLevels = ['INFO', 'WARNING', 'ERROR']
const processingMode = ['folder', 'playlist']
const outputMode = ['desktop', 'hls', 'stream', 'null']

const extensions = computed({
    get() {
        return configStore.playout.storage.extensions.join(',')
    },

    set(value: string) {
        configStore.playout.storage.extensions = value.replaceAll(' ', '').split(/,|;/)
    },
})

const formatIgnoreLines = computed({
    get() {
        return configStore.playout.logging.ignore_lines.join(';')
    },

    set(value) {
        configStore.playout.logging.ignore_lines = value.split(';')
    },
})

async function onSubmitPlayout() {
    const update = await configStore.setPlayoutConfig(configStore.playout)
    configStore.onetimeInfo = true

    if (update.status === 200) {
        indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)

        const channel = configStore.channels[configStore.i].id

        await $fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
            .then(async (response: any) => {
                if (response === 'active') {
                    configStore.showRestartModal = true
                }

                await configStore.getPlayoutConfig()
            })
            .catch((e) => {
                indexStore.msgAlert('error', e.data, 3)
            })
    } else {
        indexStore.msgAlert('error', t('config.updatePlayoutFailed'), 2)
    }
}
</script>
