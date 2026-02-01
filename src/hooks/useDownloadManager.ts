import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, DownloadCompletePayload, DownloadErrorPayload, BatchProgressPayload, DownloadFormatPreset, QueuedJob, DownloadCancelledPayload, StartDownloadResponse } from '@/types';
import { startDownload as apiStartDownload, cancelDownload as apiCancelDownload, syncDownloadState } from '@/api/invoke';
import { useAppContext } from '@/contexts/AppContext';

export function useDownloadManager() {
  const { maxConcurrentDownloads } = useAppContext();
  const [downloads, setDownloads] = useState<Map<string, Download>>(new Map());
  const hasSynced = useRef(false);

  // Consolidated update function for batching
  const updateDownloadsBatch = (updates: { jobId: string, data: Partial<Download> }[]) => {
    setDownloads((prev) => {
        const newMap = new Map(prev);
        updates.forEach(update => {
            const existing = newMap.get(update.jobId);
            if (existing) {
                // Determine if we should ignore a 'pending' update if we are already 'downloading'
                if (existing.status === 'downloading' && update.data.status === 'pending') {
                    return; 
                }
                newMap.set(update.jobId, { ...existing, ...update.data });
            } else {
                // New job from an event (rare, usually via startDownload)
                newMap.set(update.jobId, {
                    jobId: update.jobId,
                    url: update.data.filename || 'Resumed Download',
                    status: update.data.status || 'downloading',
                    progress: update.data.progress || 0,
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
    // Defect Fix #5: Smart Sync Merge
    if (!hasSynced.current) {
        hasSynced.current = true;
        syncDownloadState().then((recovered) => {
            if (recovered && recovered.length > 0) {
                setDownloads(prev => {
                    const newMap = new Map(prev);
                    recovered.forEach(remoteJob => {
                        const localJob = newMap.get(remoteJob.jobId);
                        
                        // If we already have local state for this job, we need to be careful
                        // The event listener might have already fired with newer data
                        if (localJob) {
                            // If local is 'downloading' and remote says 'pending', trust local (events are faster)
                            if (localJob.status === 'downloading' && remoteJob.status === 'pending') {
                                return;
                            }
                            // If local progress is higher, trust local
                            if (localJob.progress > remoteJob.progress) {
                                return;
                            }
                        }
                        
                        // Otherwise, trust the sync
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
                status: 'downloading' as const,
                progress: u.percentage,
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
    downloadPath: string | undefined, 
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
          downloadPath, 
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
        
        // Count active downloads to calculate optimistic concurrency slots
        const currentActiveCount = Array.from(prev.values()).filter(d => 
            d.status === 'downloading'
        ).length;
        
        let availableSlots = maxConcurrentDownloads - currentActiveCount;

        response.job_ids.forEach(jobId => {
            const existing = newMap.get(jobId);

            // If the job is already tracked and active (via event listener), we treat it as occupying a slot.
            // If it's new, we determine if we have slots available to start it immediately.
            const isAlreadyActive = existing && existing.status === 'downloading';
            
            let initialStatus: 'pending' | 'downloading' = 'pending';
            let initialPhase: string | undefined = undefined;

            if (!isAlreadyActive && availableSlots > 0) {
                initialStatus = 'downloading';
                initialPhase = 'Initializing Process...';
                availableSlots--;
            }

            if (existing) {
                // The event listener beat us to creating the entry.
                // We perform a smart merge: Backfill the static metadata (settings) while preserving
                // the dynamic runtime state (progress, status, speed) from the event.
                newMap.set(jobId, {
                    ...existing,
                    // Metadata Backfill
                    url,
                    preset: formatPreset,
                    videoResolution,
                    downloadPath,
                    filenameTemplate,
                    embedMetadata,
                    embedThumbnail,
                    restrictFilenames,
                    liveFromStart,
                    // Important: We do NOT overwrite status, progress, or phase here.
                });
            } else {
                // Standard initialization path (we beat the event listener)
                newMap.set(jobId, {
                    jobId,
                    url,
                    status: initialStatus,
                    phase: initialPhase,
                    progress: 0,
                    preset: formatPreset,
                    videoResolution,
                    downloadPath,
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
              newMap.set(job.id, {
                  jobId: job.id,
                  url: job.url,
                  status: 'pending',
                  progress: 0,
                  preset: job.format_preset,
                  videoResolution: job.video_resolution,
                  downloadPath: job.download_path,
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

    if (job.status === 'downloading' || job.status === 'pending') {
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

  return { downloads, startDownload, cancelDownload, removeDownload, importResumedJobs };
}