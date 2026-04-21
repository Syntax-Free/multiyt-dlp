import { Download } from '@/types';
import { DownloadItem } from './DownloadItem';
import { DownloadGridItem } from './DownloadGridItem';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useEffect, useState } from 'react';

interface DownloadQueueProps {
  downloads: Map<string, Download>;
  onCancel: (jobId: string) => void;
  viewMode: 'list' | 'grid';
}

export function DownloadQueue({ downloads, onCancel, viewMode }: DownloadQueueProps) {
  const downloadArray = Array.from(downloads.values());
  const [scrollEl, setScrollEl] = useState<Element | null>(null);

  useEffect(() => {
      // Find the scroll container established in Layout.tsx
      setScrollEl(document.getElementById('scroll-container'));
  }, []);

  const rowVirtualizer = useVirtualizer({
      count: viewMode === 'list' && scrollEl ? downloadArray.length : 0,
      getScrollElement: () => scrollEl,
      estimateSize: () => 140, // Height of list item + spacing
      overscan: 5,
  });

  if (downloadArray.length === 0) {
    return (
      <div className="text-center text-zinc-500 py-10">
        <p>No downloads yet.</p>
        <p>Paste a URL above to get started.</p>
      </div>
    );
  }

  if (viewMode === 'grid') {
      // Grid view utilizes decoupled state, rendering 300+ items is lightweight since they never re-render
      return (
        <div className="grid grid-cols-4 sm:grid-cols-5 md:grid-cols-6 lg:grid-cols-8 gap-3 animate-fade-in">
            {downloadArray.map((download) => (
                <DownloadGridItem
                    key={download.jobId}
                    download={download}
                    onCancel={onCancel}
                />
            ))}
        </div>
      );
  }

  // Virtualized List View
  return (
    <div 
        className="relative w-full"
        style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
    >
      {rowVirtualizer.getVirtualItems().map((virtualRow) => {
          const download = downloadArray[virtualRow.index];
          return (
              <div
                  key={download.jobId}
                  className="absolute top-0 left-0 w-full"
                  style={{
                      height: `${virtualRow.size}px`,
                      transform: `translateY(${virtualRow.start}px)`,
                  }}
              >
                  {/* Padding bottom replaces gap-2 */}
                  <div className="pb-2">
                      <DownloadItem download={download} onCancel={onCancel} />
                  </div>
              </div>
          );
      })}
    </div>
  );
}