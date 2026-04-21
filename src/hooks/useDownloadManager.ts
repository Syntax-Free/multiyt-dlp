import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, DownloadCompletePayload, DownloadErrorPayload, BatchProgressPayload, DownloadFormatPreset, QueuedJob, DownloadCancelledPayload, StartDownloadResponse, DownloadStatus } from '@/types';
import { startDownload as apiStartDownload, cancelDownload as apiCancelDownload, resolveFileConflict as apiResolveConflict, syncDownloadState } from '@/api/invoke';
import { useAppContext } from '@/contexts/AppContext';

// --- DECOUPLED PROGRESS PUB/SUB ---
export type ProgressData = {
    progress?: number;
    speed?: string;
    eta?: string;
    phase?: string;
    status?: DownloadStatus;
};

class ProgressEmitter {
    private store = new Map<string, ProgressData>();
    private listeners = new Map<string, Set<(data: ProgressData) => void>>();

    emit(jobId: string, data: ProgressData) {
        const current = this.store.get(jobId) || {};
        const next = { ...current, ...data };
        this.store.set(jobId, next);
        this.listeners.get(jobId)?.forEach(cb => cb(next));
    }

    subscribe(jobId: string, cb: (data: ProgressData) => void) {
        if (!this.listeners.has(jobId)) this.listeners.set(jobId, new Set());
        this.listeners.get(jobId)!.add(cb);
        if (this.store.has(jobId)) {
            cb(this.store.get(jobId)!);
        }
    }

    unsubscribe(jobId: string, cb: (data: ProgressData) => void) {
        this.listeners.get(jobId)?.delete(cb);
    }

    get(jobId: string) {
        return this.store.get(jobId);
    }
}

export const progressEmitter = new ProgressEmitter();
// -----------------------------------

export function useDownloadManager() {
  const { maxConcurrentDownloads } = useAppContext();
  const [downloads, setDownloads] = useState<Map<string, Download>>(new Map());
  const hasSynced = useRef(false);
  const downloadsRef = useRef(downloads);

  // Keep ref in sync for O(1) state diffing without triggering re-renders in the effect
  useEffect(() => {
      downloadsRef.current = downloads;
  }, [downloads]);

  const updateDownloadsBatch = useCallback((updates: { jobId: string, data: Partial<Download> }[]) => {
    setDownloads((prev) => {
        const newMap = new Map(prev);
        updates.forEach(update => {
            const existing = newMap.get(update.jobId);
            
            if (existing) {
                if (update.data.sequence_id !== undefined && existing.sequence_id > update.data.sequence_id) {
                    return;
                }
                const mergedFilename = update.data.filename || existing.filename;
                newMap.set(update.jobId, { 
                    ...existing, 
                    ...update.data,
                    filename: mergedFilename 
                });
            } else {
                newMap.set(update.jobId, {
                    jobId: update.jobId,
                    url: update.data.filename || 'Resumed Download',
                    status: update.data.status || 'downloading',
                    progress: update.data.progress || 0,
                    sequence_id: update.data.sequence_id || 0,
                    ...update.data
                } as Download);
            }
        });
        return newMap;
    });
  }, []);

  const updateDownload = useCallback((jobId: string, newProps: Partial<Download>) => {
      setDownloads((prev) => {
          const newMap = new Map(prev);
          const existing = newMap.get(jobId);
          if (existing) {
              const nextSequence = existing.sequence_id + 1;
              const mergedFilename = newProps.filename || existing.filename;
              newMap.set(jobId, {
                  ...existing,
                  ...newProps,
                  sequence_id: Math.max(existing.sequence_id, nextSequence),
                  filename: mergedFilename
              });
          }
          return newMap;
      });
  }, []);

  useEffect(() => {
    if (!hasSynced.current) {
        hasSynced.current = true;
        syncDownloadState().then((recovered) => {
            if (recovered && recovered.length > 0) {
                setDownloads(prev => {
                    const newMap = new Map(prev);
                    recovered.forEach(remoteJob => {
                        const localJob = newMap.get(remoteJob.jobId);
                        if (localJob && localJob.sequence_id > remoteJob.sequence_id) {
                            return;
                        }
                        newMap.set(remoteJob.jobId, remoteJob);
                    });
                    return newMap;
                });
            }
        }).catch(console.error);
    }

    const unlistenProgress = listen<BatchProgressPayload>('download-progress-batch', (event) => {
        let needsGlobalUpdate = false;
        const globalUpdates: { jobId: string, data: Partial<Download> }[] = [];

        event.payload.updates.forEach(u => {
            const currentGlobal = downloadsRef.current.get(u.jobId);
            
            // 1. Emit locally (Zero React Overhead)
            progressEmitter.emit(u.jobId, {
                progress: u.percentage,
                speed: u.speed,
                eta: u.eta,
                phase: u.phase || undefined,
                status: u.status || undefined,
            });

            // 2. Diff for Global State (Only trigger React if structural status changes)
            if (currentGlobal) {
                const statusChanged = u.status && u.status !== currentGlobal.status;
                const filenameChanged = u.filename && u.filename !== currentGlobal.filename;
                
                if (statusChanged || filenameChanged) {
                    needsGlobalUpdate = true;
                    globalUpdates.push({
                        jobId: u.jobId,
                        data: {
                            status: u.status || currentGlobal.status,
                            filename: u.filename || currentGlobal.filename,
                            phase: u.phase || currentGlobal.phase,
                            sequence_id: u.sequence_id
                        }
                    });
                }
            }
        });

        if (needsGlobalUpdate && globalUpdates.length > 0) {
            updateDownloadsBatch(globalUpdates);
        }
    });

    const unlistenComplete = listen<DownloadCompletePayload>('download-complete', (event) => {
      progressEmitter.emit(event.payload.jobId, { status: event.payload.status || 'completed', progress: 100, phase: 'Done' });
      updateDownload(event.payload.jobId, {
        status: event.payload.status || 'completed',
        progress: 100,
        outputPath: event.payload.outputPath,
        phase: 'Done',
        usedCommand: event.payload.usedCommand,
      });
    });

    const unlistenError = listen<DownloadErrorPayload>('download-error', (event) => {
      progressEmitter.emit(event.payload.jobId, { status: 'error' });
      updateDownload(event.payload.jobId, {
        status: 'error',
        error: event.payload.error,
        exit_code: event.payload.exit_code,
        stderr: event.payload.stderr,
        logs: event.payload.logs,
      });
    });

    const unlistenCancelled = listen<DownloadCancelledPayload>('download-cancelled', (event) => {
        progressEmitter.emit(event.payload.jobId, { status: 'cancelled', phase: 'Cancelled by user', eta: '--', speed: '--' });
        updateDownload(event.payload.jobId, {
            status: 'cancelled',
            phase: 'Cancelled by user',
            eta: '--',
            speed: '--'
        });
    });

    return () => {
      unlistenProgress.then((f) => f());
      unlistenComplete.then((f) => f());
      unlistenError.then((f) => f());
      unlistenCancelled.then((f) => f());
    };
  }, [updateDownloadsBatch, updateDownload]);

  const startDownload = useCallback(async (
    url: string, 
    downloadPath: string | null | undefined, 
    formatPreset: DownloadFormatPreset = 'best',
    videoResolution: string,
    embedMetadata: boolean = false,
    embedThumbnail: boolean = false,
    filenameTemplate: string,
    restrictFilenames: boolean = false,
    forceDownload: boolean = false,
    urlWhitelist?: string[],
    liveFromStart: boolean = false,
    downloadSections?: string
  ): Promise<StartDownloadResponse> => {
    try {
      const response = await apiStartDownload(
          url, 
          downloadPath ?? undefined, 
          formatPreset,
          videoResolution, 
          embedMetadata, 
          embedThumbnail,
          filenameTemplate,
          restrictFilenames,
          forceDownload,
          urlWhitelist,
          liveFromStart,
          downloadSections
      ); 
      
      setDownloads((prev) => {
        const newMap = new Map(prev);
        
        response.job_ids.forEach(jobId => {
            const existing = newMap.get(jobId);

            if (existing) {
                newMap.set(jobId, {
                    ...existing,
                    url,
                    preset: formatPreset,
                    videoResolution,
                    downloadPath: downloadPath ?? undefined,
                    filenameTemplate,
                    embedMetadata,
                    embedThumbnail,
                    restrictFilenames,
                    liveFromStart,
                    downloadSections,
                });
            } else {
                newMap.set(jobId, {
                    jobId,
                    url,
                    status: 'pending',
                    phase: 'Queued',
                    progress: 0,
                    sequence_id: 0,
                    preset: formatPreset,
                    videoResolution,
                    downloadPath: downloadPath ?? undefined,
                    filenameTemplate,
                    embedMetadata,
                    embedThumbnail,
                    restrictFilenames,
                    liveFromStart,
                    downloadSections,
                });
            }
        });
        return newMap;
    });

      return response;
    } catch (error) {
      console.error('Failed to start download:', error);
      throw error;
    }
  }, [maxConcurrentDownloads]);

  const importResumedJobs = useCallback((jobs: QueuedJob[]) => {
      setDownloads((prev) => {
          const newMap = new Map(prev);
          jobs.forEach(job => {
              let initialStatus: 'pending' | 'error' = 'pending';
              let initialError: string | undefined;
              let initialStderr: string | undefined;

              if (job.status === 'error') {
                  initialStatus = 'error';
                  initialError = job.error || "Unknown Error";
                  initialStderr = job.stderr;
              }

              newMap.set(job.id, {
                  jobId: job.id,
                  url: job.url,
                  status: initialStatus,
                  error: initialError,
                  stderr: initialStderr,
                  progress: 0,
                  sequence_id: 0,
                  preset: job.format_preset,
                  videoResolution: job.video_resolution,
                  downloadPath: job.download_path ?? undefined,
                  filenameTemplate: job.filename_template,
                  embedMetadata: job.embed_metadata,
                  embedThumbnail: job.embed_thumbnail,
                  restrictFilenames: job.restrict_filenames,
                  liveFromStart: job.live_from_start,
                  downloadSections: job.download_sections
              });
          });
          return newMap;
      });
  },[]);

  const removeDownload = useCallback((jobId: string) => {
      setDownloads((prev) => {
          const newMap = new Map(prev);
          newMap.delete(jobId);
          return newMap;
      });
  },[]);

  const cancelDownload = useCallback(async (jobId: string) => {
    const job = downloads.get(jobId);
    if (!job) return;

    if (job.status === 'downloading' || job.status === 'pending' || job.status === 'file_conflict') {
        try {
            updateDownload(jobId, { status: 'cancelled', phase: 'Cancelling...' });
            await apiCancelDownload(jobId);
        } catch (error) {
            console.error('Failed to cancel download:', error);
            updateDownload(jobId, { status: 'error', error: 'Failed to cancel.' });
        }
    } else {
        removeDownload(jobId);
    }
  }, [downloads, removeDownload, updateDownload]);

  const cancelAllDownloads = useCallback(async () => {
      const targets = Array.from(downloads.values()).filter(d => 
          d.status === 'downloading' || d.status === 'pending'
      );
      
      for (const job of targets) {
          try {
              updateDownload(job.jobId, { status: 'cancelled', phase: 'Cancelling...' });
              await apiCancelDownload(job.jobId);
          } catch (e) {
              console.error(`Bulk cancel failed for ${job.jobId}`, e);
          }
      }
  }, [downloads, updateDownload]);

  const resolveConflict = useCallback(async (jobId: string, resolution: 'overwrite' | 'discard') => {
      try {
          updateDownload(jobId, { phase: resolution === 'overwrite' ? 'Overwriting...' : 'Discarding...' });
          await apiResolveConflict(jobId, resolution);
      } catch (err) {
          console.error("Failed to resolve conflict", err);
          updateDownload(jobId, { status: 'error', error: 'Failed to resolve conflict' });
      }
  }, [updateDownload]);

  return { downloads, startDownload, cancelDownload, removeDownload, importResumedJobs, cancelAllDownloads, resolveConflict };
}