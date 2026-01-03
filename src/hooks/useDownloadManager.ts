import { useState, useEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, DownloadCompletePayload, DownloadErrorPayload, BatchProgressPayload, DownloadFormatPreset, QueuedJob, DownloadCancelledPayload, StartDownloadResponse } from '@/types';
import { startDownload as apiStartDownload, cancelDownload as apiCancelDownload } from '@/api/invoke';
import { useAppContext } from '@/contexts/AppContext';

export function useDownloadManager() {
  const { maxConcurrentDownloads } = useAppContext();
  const [downloads, setDownloads] = useState<Map<string, Download>>(new Map());

  // Consolidated update function for batching
  const updateDownloadsBatch = (updates: { jobId: string, data: Partial<Download> }[]) => {
    setDownloads((prev) => {
        const newMap = new Map(prev);
        updates.forEach(update => {
            const existing = newMap.get(update.jobId);
            if (existing) {
                // Determine if we should ignore a 'pending' update if we are already 'downloading'
                // This prevents race conditions where an old backend message reverts our optimistic UI
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
    urlWhitelist?: string[]
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
          urlWhitelist
      ); 
      
      setDownloads((prev) => {
        const newMap = new Map(prev);
        
        // --- Optimistic UI Update ---
        // Calculate how many jobs are currently active to determine which new jobs
        // should be displayed as "Initializing" vs "Queued".
        const currentActiveCount = Array.from(prev.values()).filter(d => 
            d.status === 'downloading'
        ).length;
        
        let availableSlots = maxConcurrentDownloads - currentActiveCount;

        response.job_ids.forEach(jobId => {
            let initialStatus: 'pending' | 'downloading' = 'pending';
            let initialPhase: string | undefined = undefined;

            // If we have slots, optimistically set to downloading/initializing
            if (availableSlots > 0) {
                initialStatus = 'downloading';
                initialPhase = 'Initializing Process...';
                availableSlots--;
            }

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
              restrictFilenames
            });
        });
        return newMap;
      });

      return response;
    } catch (error) {
      console.error('Failed to start download:', error);
      throw error;
    }
  }, [maxConcurrentDownloads]); // Added dependency

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
                  restrictFilenames: job.restrict_filenames
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