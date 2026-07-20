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
    const uploadBatches = new Map<string, string>()

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
        loadedBytes: number,
        fileSize: number,
        currentIndex: number,
        batchCount: number,
    ) {
        const now = Date.now()
        const confirmedBytes = Math.min(loadedBytes, fileSize)
        const deltaBytes = confirmedBytes - lastLoaded
        const deltaTime = (now - lastTime.value) / 1000
        const instantSpeed = deltaTime > 0 ? deltaBytes / deltaTime : 0

        speedHistory.push(instantSpeed)
        if (speedHistory.length > MAX_HISTORY) {
            speedHistory.shift()
        }

        const currentFileProgress = fileSize > 0 ? confirmedBytes / fileSize : 0
        const totalProgress = batchCount > 0 ? (currentIndex + currentFileProgress) / batchCount : 0

        currentProgress.value = Math.round(totalProgress * 100)
        lastLoaded = confirmedBytes
        lastTime.value = now
    }

    async function getUploadStatus(
        file: globalThis.File,
        request: UploadRequestOptions,
        batchId: string,
    ): Promise<{ received_ranges: [number, number][] }> {
        const url = new URL(request.url, window.location.origin)
        url.searchParams.set('file_name', file.name)
        url.searchParams.set('size', file.size.toString())
        url.searchParams.set('batch_id', batchId)

        const response = await fetch(url, {
            headers: request.headers,
        })
        if (!response.ok) {
            throw new Error(await errMsg(response))
        }

        return (await response.json()) as {
            received_ranges: [number, number][]
        }
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
        const fileSize = Number(file.size)
        const uploadKey = `${request.url}\0${file.name}\0${fileSize}\0${file.lastModified}`
        const batchId = request.batchId || uploadBatches.get(uploadKey) || crypto.randomUUID()
        uploadBatches.set(uploadKey, batchId)
        const status = await getUploadStatus(file, request, batchId)

        let offset = 0
        let completedBytes = 0
        let lastResponse: T | undefined

        const queue: { start: number; end: number; blob: Blob }[] = []

        while (offset < fileSize) {
            const end = Math.min(offset + chunkSize, fileSize)
            const alreadyReceived = status.received_ranges.some(
                ([rangeStart, rangeEnd]) => rangeStart <= offset && rangeEnd >= end,
            )
            if (alreadyReceived) {
                completedBytes += end - offset
            } else {
                queue.push({ start: offset, end, blob: file.slice(offset, end) as Blob })
            }
            offset = end
        }
        updateProgress(completedBytes, fileSize, currentIndex, batchCount)

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

                completedBytes += nextChunk.end - nextChunk.start
                updateProgress(completedBytes, fileSize, currentIndex, batchCount)
                lastResponse = await parseResponse<T>(resp, request.responseType)
            }
        }

        const workers = Array(Math.min(parallelUploads, queue.length))
            .fill(0)
            .map(() => worker())

        await Promise.all(workers)
        uploadBatches.delete(uploadKey)

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
