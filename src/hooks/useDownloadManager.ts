import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, DownloadCompletePayload, DownloadErrorPayload, BatchProgressPayload, DownloadFormatPreset, QueuedJob, DownloadCancelledPayload, StartDownloadResponse } from '@/types';
import { startDownload as apiStartDownload, cancelDownload as apiCancelDownload, resolveFileConflict as apiResolveConflict, syncDownloadState } from '@/api/invoke';
import { useAppContext } from '@/contexts/AppContext';

export function useDownloadManager() {
  const { maxConcurrentDownloads } = useAppContext();
  const [downloads, setDownloads] = useState<Map<string, Download>>(new Map());
  const hasSynced = useRef(false);

  // Consolidated update function for batching with Sequence ID checking
  const updateDownloadsBatch = (updates: { jobId: string, data: Partial<Download> }[]) => {
    setDownloads((prev) => {
        const newMap = new Map(prev);
        updates.forEach(update => {
            const existing = newMap.get(update.jobId);
            
            if (existing) {
                // SEQUENCE CHECK: Ignore out-of-order updates
                if (update.data.sequence_id !== undefined && existing.sequence_id > update.data.sequence_id) {
                    return;
                }

                // If the update payload has no filename, but the existing state DOES, preserve it.
                const mergedFilename = update.data.filename || existing.filename;

                newMap.set(update.jobId, { 
                    ...existing, 
                    ...update.data,
                    filename: mergedFilename 
                });
            } else {
                // New job from an event
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
  };

  const updateDownload = (jobId: string, newProps: Partial<Download>) => {
      updateDownloadsBatch([{ jobId, data: newProps }]);
  };

  useEffect(() => {
    // 1. Recover state on mount (UI Refresh resilience)
    if (!hasSynced.current) {
        hasSynced.current = true;
        syncDownloadState().then((recovered) => {
            if (recovered && recovered.length > 0) {
                setDownloads(prev => {
                    const newMap = new Map(prev);
                    recovered.forEach(remoteJob => {
                        const localJob = newMap.get(remoteJob.jobId);
                        
                        // ZOMBIE FIX: We trust the backend state regarding status.
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
        const updates = event.payload.updates.map(u => ({
            jobId: u.jobId,
            data: {
                status: u.status || 'downloading', 
                progress: u.percentage,
                sequence_id: u.sequence_id,
                speed: u.speed,
                eta: u.eta,
                filename: u.filename,
                phase: u.phase
            }
        }));
        updateDownloadsBatch(updates);
    });

    const unlistenComplete = listen<DownloadCompletePayload>('download-complete', (event) => {
      updateDownload(event.payload.jobId, {
        status: 'completed',
        progress: 100,
        outputPath: event.payload.outputPath,
        phase: 'Done',
      });
    });

    const unlistenError = listen<DownloadErrorPayload>('download-error', (event) => {
      updateDownload(event.payload.jobId, {
        status: 'error',
        error: event.payload.error,
        exit_code: event.payload.exit_code,
        stderr: event.payload.stderr,
        logs: event.payload.logs,
      });
    });

    const unlistenCancelled = listen<DownloadCancelledPayload>('download-cancelled', (event) => {
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
  }, []);

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
    liveFromStart: boolean = false
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
          liveFromStart
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
                    liveFromStart
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
                  liveFromStart: job.live_from_start
              });
          });
          return newMap;
      });
  }, []);

  const removeDownload = useCallback((jobId: string) => {
      setDownloads((prev) => {
          const newMap = new Map(prev);
          newMap.delete(jobId);
          return newMap;
      });
  }, []);

  const cancelDownload = useCallback(async (jobId: string) => {
    const job = downloads.get(jobId);
    if (!job) return;

    if (job.status === 'downloading' || job.status === 'pending' || job.status === 'file_conflict') {
        try {
            await apiCancelDownload(jobId);
            updateDownload(jobId, { status: 'cancelled', phase: 'Cancelling...' });
        } catch (error) {
            console.error('Failed to cancel download:', error);
            updateDownload(jobId, { status: 'error', error: 'Failed to cancel.' });
        }
    } else {
        removeDownload(jobId);
    }
  }, [downloads, removeDownload]);

  const cancelAllDownloads = useCallback(async () => {
      const targets = Array.from(downloads.values()).filter(d => 
          d.status === 'downloading' || d.status === 'pending'
      );
      
      for (const job of targets) {
          try {
              await apiCancelDownload(job.jobId);
              updateDownload(job.jobId, { status: 'cancelled', phase: 'Cancelling...' });
          } catch (e) {
              console.error(`Bulk cancel failed for ${job.jobId}`, e);
          }
      }
  }, [downloads]);

  const resolveConflict = useCallback(async (jobId: string, resolution: 'overwrite' | 'discard') => {
      try {
          // Optimistically update UI
          updateDownload(jobId, { phase: resolution === 'overwrite' ? 'Overwriting...' : 'Discarding...' });
          await apiResolveConflict(jobId, resolution);
      } catch (err) {
          console.error("Failed to resolve conflict", err);
          updateDownload(jobId, { status: 'error', error: 'Failed to resolve conflict' });
      }
  }, []);

  return { downloads, startDownload, cancelDownload, removeDownload, importResumedJobs, cancelAllDownloads, resolveConflict };
}