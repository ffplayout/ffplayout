import { ref } from 'vue'

import { useHelper } from '@/composables/helper'

interface UploadRequestOptions {
    url: string
    headers?: HeadersInit
    method?: 'POST' | 'PUT'
    extraFormData?: Record<string, string>
    responseType?: 'json' | 'text'
    chunkSize?: number
    parallelUploads?: number
    currentIndex?: number
    batchCount?: number
    batchId?: string
}

interface UploadFilesOptions<T = unknown> {
    buildRequest: (file: globalThis.File, currentIndex: number, batchCount: number) => UploadRequestOptions
    onFileComplete?: (response: T, file: globalThis.File, currentIndex: number) => void | Promise<void>
}

const DEFAULT_CHUNK_SIZE = 1024 * 512
const DEFAULT_PARALLEL_UPLOADS = 4
const MAX_HISTORY = 5

export const useFileUpload = () => {
    const { errMsg } = useHelper()

    const uploadTask = ref('')
    const currentNumber = ref(0)
    const currentProgress = ref(0)
    const overallProgress = ref(0)
    const uploading = ref(false)
    const error = ref('')

    const speedHistory: number[] = []

    let lastLoaded = 0
    const lastTime = ref(Date.now())

    function resetProgressTracking() {
        lastLoaded = 0
        lastTime.value = Date.now()
        speedHistory.length = 0
    }

    function resetUploadState() {
        uploadTask.value = ''
        currentNumber.value = 0
        currentProgress.value = 0
        overallProgress.value = 0
        uploading.value = false
        error.value = ''
        resetProgressTracking()
    }

    function updateProgress(
        completedChunks: number,
        fileSize: number,
        currentIndex: number,
        batchCount: number,
        chunkSize: number,
    ) {
        const now = Date.now()
        const loadedBytes = Math.min(completedChunks * chunkSize, fileSize)
        const deltaBytes = loadedBytes - lastLoaded
        const deltaTime = (now - lastTime.value) / 1000
        const instantSpeed = deltaTime > 0 ? deltaBytes / deltaTime : 0

        speedHistory.push(instantSpeed)
        if (speedHistory.length > MAX_HISTORY) {
            speedHistory.shift()
        }

        const currentFileProgress = fileSize > 0 ? loadedBytes / fileSize : 0
        const totalProgress = batchCount > 0 ? (currentIndex + currentFileProgress) / batchCount : 0

        currentProgress.value = Math.round(totalProgress * 100)
        lastLoaded = loadedBytes
        lastTime.value = now
    }

    async function parseResponse<T>(resp: Response, responseType: 'json' | 'text' = 'text'): Promise<T> {
        if (responseType === 'json') {
            return (await resp.json()) as T
        }

        return (await resp.text()) as T
    }

    async function uploadChunkedFile<T>(file: globalThis.File, request: UploadRequestOptions): Promise<T | undefined> {
        const chunkSize = request.chunkSize || DEFAULT_CHUNK_SIZE
        const parallelUploads = request.parallelUploads || DEFAULT_PARALLEL_UPLOADS
        const currentIndex = request.currentIndex || 0
        const batchCount = request.batchCount || 1
        const batchId = request.batchId || crypto.randomUUID()
        const fileSize = Number(file.size)

        let offset = 0
        let completedChunks = 0
        let lastResponse: T | undefined

        const totalChunks = Math.ceil(fileSize / chunkSize)
        const queue: { start: number; end: number; blob: Blob }[] = []

        while (offset < fileSize) {
            const end = Math.min(offset + chunkSize, fileSize)
            queue.push({ start: offset, end, blob: file.slice(offset, end) as Blob })
            offset = end
        }

        async function worker() {
            while (queue.length) {
                const nextChunk = queue.shift()

                if (!nextChunk) {
                    return
                }

                const form = new FormData()
                form.append('fileName', file.name)
                form.append('start', nextChunk.start.toString())
                form.append('end', nextChunk.end.toString())
                form.append('size', fileSize.toString())
                form.append('chunk', nextChunk.blob)
                form.append('batch_id', batchId)

                if (request.extraFormData) {
                    for (const [key, value] of Object.entries(request.extraFormData)) {
                        form.append(key, value)
                    }
                }

                const resp = await fetch(request.url, {
                    method: request.method || 'PUT',
                    headers: request.headers,
                    body: form,
                })

                if (!resp.ok) {
                    throw new Error(await errMsg(resp))
                }

                completedChunks++
                updateProgress(completedChunks, fileSize, currentIndex, batchCount, chunkSize)
                lastResponse = await parseResponse<T>(resp, request.responseType)
            }
        }

        const workers = Array(Math.min(parallelUploads, totalChunks))
            .fill(0)
            .map(() => worker())

        await Promise.all(workers)

        return lastResponse
    }

    async function uploadFile<T = string>(
        file: globalThis.File,
        request: UploadRequestOptions,
    ): Promise<T | undefined> {
        resetProgressTracking()
        currentProgress.value = 0

        return uploadChunkedFile<T>(file, request)
    }

    async function uploadFiles<T = string>(files: globalThis.File[], options: UploadFilesOptions<T>): Promise<T[]> {
        const responses: T[] = []

        if (!files.length) {
            resetUploadState()
            return responses
        }

        uploading.value = true
        error.value = ''

        try {
            for (let index = 0; index < files.length; index++) {
                const file = files[index]

                if (!file) {
                    continue
                }

                uploadTask.value = file.name
                currentNumber.value = index + 1
                currentProgress.value = 0

                try {
                    const response = await uploadFile<T>(file, options.buildRequest(file, index, files.length))

                    if (response !== undefined) {
                        responses.push(response)

                        if (options.onFileComplete) {
                            await options.onFileComplete(response, file, index)
                        }
                    }
                } catch (err: any) {
                    error.value = err.message || 'Upload failed'
                    throw err
                } finally {
                    overallProgress.value = ((index + 1) * 100) / files.length
                }
            }
        } finally {
            uploading.value = false
        }

        return responses
    }

    return {
        uploadTask,
        currentNumber,
        currentProgress,
        overallProgress,
        uploading,
        error,
        resetUploadState,
        uploadFile,
        uploadFiles,
    }
}
