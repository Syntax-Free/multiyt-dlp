// src/components/DownloadQueue.tsx
import { Download } from '@/types';
import { DownloadItem } from './DownloadItem';
import { DownloadGridItem } from './DownloadGridItem';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useEffect, useState, useRef } from 'react';

interface DownloadQueueProps {
  downloads: Map<string, Download>;
  onCancel: (jobId: string) => void;
  viewMode: 'list' | 'grid';
}

/**
 * Helper: determine column count for grid based on window width.
 * matches the responsive classes originally used.
 */
function getColumnCount(): number {
  if (typeof window === 'undefined') return 8; // SSR fallback
  const w = window.innerWidth;
  if (w < 640) return 4;   // sm
  if (w < 768) return 5;   // md
  if (w < 1024) return 6;  // lg
  return 8;                // xl
}

export function DownloadQueue({ downloads, onCancel, viewMode }: DownloadQueueProps) {
  const downloadArray = Array.from(downloads.values());
  const [scrollEl, setScrollEl] = useState<Element | null>(null);
  const [colCount, setColCount] = useState(getColumnCount);

  // Capture scroll container once
  useEffect(() => {
    const el = document.getElementById('scroll-container');
    setScrollEl(el ?? null);
  }, []);

  // Keep column count reactive for grid responsiveness
  useEffect(() => {
    const handleResize = () => setColCount(getColumnCount());
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  // ---- Virtualizer for LIST view (dynamic heights) ----
  const listVirtualizer = useVirtualizer({
    count: viewMode === 'list' && scrollEl ? downloadArray.length : 0,
    getScrollElement: () => scrollEl,
    estimateSize: () => 120,           // initial guess
    measureElement: (el) => el.getBoundingClientRect().height,
    getItemKey: (index: number) => downloadArray[index].jobId,
    overscan: 5,
  });

  // ---- Virtualizer for GRID view (row‑based) ----
  const rowCount = viewMode === 'grid' ? Math.ceil(downloadArray.length / colCount) : 0;
  const gridVirtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollEl,
    estimateSize: () => 180,           // approximate row height (aspect‑square items + gap)
    measureElement: (el) => el.getBoundingClientRect().height,
    getItemKey: (index: number) => `grid-row-${index}`,
    overscan: 2,
  });

  // After downloads change (any property that may affect layout), recalc virtual sizes.
  const prevDownloadsRef = useRef(downloads);
  useEffect(() => {
    if (viewMode === 'list' && listVirtualizer) {
      listVirtualizer.measure();
    } else if (viewMode === 'grid' && gridVirtualizer) {
      gridVirtualizer.measure();
    }
    prevDownloadsRef.current = downloads;
  }, [downloads, listVirtualizer, gridVirtualizer, viewMode]);

  if (downloadArray.length === 0) {
    return (
      <div className="text-center text-zinc-500 py-10">
        <p>No downloads yet.</p>
        <p>Paste a URL above to get started.</p>
      </div>
    );
  }

  // ------------------------------------------------------------------
  // GRID VIEW – virtualized rows
  // ------------------------------------------------------------------
  if (viewMode === 'grid') {
    return (
      <div
        className="relative w-full"
        style={{ height: `${gridVirtualizer.getTotalSize()}px` }}
      >
        {gridVirtualizer.getVirtualItems().map((virtualRow) => {
          const startIdx = virtualRow.index * colCount;
          const rowItems = downloadArray.slice(startIdx, startIdx + colCount);

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={gridVirtualizer.measureElement}
              className="absolute top-0 left-0 w-full"
              style={{
                height: `${virtualRow.size}px`,
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              <div
                className="grid gap-3 h-full"
                style={{
                  gridTemplateColumns: `repeat(${colCount}, 1fr)`,
                }}
              >
                {rowItems.map((download) => (
                  <DownloadGridItem
                    key={download.jobId}
                    download={download}
                    onCancel={onCancel}
                  />
                ))}
              </div>
            </div>
          );
        })}
      </div>
    );
  }

  // ------------------------------------------------------------------
  // LIST VIEW – virtualized with dynamic measurement
  // ------------------------------------------------------------------
  return (
    <div
      className="relative w-full"
      style={{ height: `${listVirtualizer.getTotalSize()}px` }}
    >
      {listVirtualizer.getVirtualItems().map((virtualRow) => {
        const download = downloadArray[virtualRow.index];
        return (
          <div
            key={virtualRow.key}
            data-index={virtualRow.index}
            ref={listVirtualizer.measureElement}
            className="absolute top-0 left-0 w-full"
            style={{
              transform: `translateY(${virtualRow.start}px)`,
            }}
          >
            {/* margin-bottom provides spacing without breaking dynamic measurement */}
            <div className="mb-3">
              <DownloadItem download={download} onCancel={onCancel} />
            </div>
          </div>
        );
      })}
    </div>
  );
}