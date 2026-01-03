import { useState, useEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, DownloadCompletePayload, DownloadErrorPayload, BatchProgressPayload, DownloadFormatPreset, QueuedJob, DownloadCancelledPayload, StartDownloadResponse } from '@/types';
import { startDownload as apiStartDownload, cancelDownload as apiCancelDownload } from '@/api/invoke';

export function useDownloadManager() {
  const [downloads, setDownloads] = useState<Map<string, Download>>(new Map());

  // Consolidated update function for batching
  const updateDownloadsBatch = (updates: { jobId: string, data: Partial<Download> }[]) => {
    setDownloads((prev) => {
        const newMap = new Map(prev);
        updates.forEach(update => {
            const existing = newMap.get(update.jobId);
            if (existing) {
                newMap.set(update.jobId, { ...existing, ...update.data });
            } else {
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

    // NEW: Listen for Clean Cancellation
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
        response.job_ids.forEach(jobId => {
            newMap.set(jobId, {
              jobId,
              url,
              status: 'pending',
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
  }, []);

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

  // SMART CANCEL: If Active -> Cancel API. If Inactive -> Remove from Map.
  const cancelDownload = useCallback(async (jobId: string) => {
    // We need to look up the current state of the job.
    // Since 'downloads' state is in the closure, we use functional update to ensure consistency,
    // but here we need to READ the value to decide logic.
    // Note: In React 18+ auto-batching helps, but technically reading `downloads` directly here 
    // relies on the closure being fresh. `cancelDownload` includes [downloads] in dependency array.
    
    const job = downloads.get(jobId);
    if (!job) return;

    if (job.status === 'downloading' || job.status === 'pending') {
        // Active: Call API to kill process.
        try {
            await apiCancelDownload(jobId);
            // We can optimistically set it, but backend will send 'download-cancelled' event too.
            updateDownload(jobId, { status: 'cancelled', phase: 'Cancelling...' });
        } catch (error) {
            console.error('Failed to cancel download:', error);
            updateDownload(jobId, { status: 'error', error: 'Failed to cancel.' });
        }
    } else {
        // Inactive (Completed, Error, Cancelled): Remove from UI.
        removeDownload(jobId);
    }
  }, [downloads, removeDownload]);

  return { downloads, startDownload, cancelDownload, removeDownload, importResumedJobs };
}