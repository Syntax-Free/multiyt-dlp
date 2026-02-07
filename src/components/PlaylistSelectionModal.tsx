import { useState, useMemo } from 'react';
import { Modal } from './ui/Modal';
import { Button } from './ui/Button';
import { PlaylistEntry } from '@/types';
import { Search, CheckSquare, Square, Download, X, ListFilter } from 'lucide-react';
import { twMerge } from 'tailwind-merge';

interface PlaylistSelectionModalProps {
    isOpen: boolean;
    onClose: () => void;
    entries: PlaylistEntry[];
    onConfirm: (selectedUrls: string[]) => void;
    title?: string;
}

export function PlaylistSelectionModal({ isOpen, onClose, entries, onConfirm, title }: PlaylistSelectionModalProps) {
    const [search, setSearch] = useState('');
    const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set(entries.map(e => e.url)));

    const filteredEntries = useMemo(() => {
        return entries.filter(e => e.title.toLowerCase().includes(search.toLowerCase()));
    }, [entries, search]);

    const handleToggleAll = () => {
        if (selectedIds.size === entries.length) {
            setSelectedIds(new Set());
        } else {
            setSelectedIds(new Set(entries.map(e => e.url)));
        }
    };

    const handleToggleEntry = (url: string) => {
        const next = new Set(selectedIds);
        if (next.has(url)) next.delete(url);
        else next.add(url);
        setSelectedIds(next);
    };

    const handleConfirm = () => {
        onConfirm(Array.from(selectedIds));
        onClose();
    };

    if (!isOpen) return null;

    return (
        <Modal 
            isOpen={isOpen} 
            onClose={onClose} 
            title={title || "Playlist Selection"}
        >
            <div className="flex flex-col h-full max-h-[70vh]">
                {/* Search & Actions Bar */}
                <div className="flex flex-col gap-4 mb-6">
                    <div className="relative group">
                        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-zinc-500 group-focus-within:text-theme-cyan transition-colors" />
                        <input 
                            type="text"
                            placeholder="Search in playlist..."
                            value={search}
                            onChange={(e) => setSearch(e.target.value)}
                            className="w-full bg-zinc-950 border border-zinc-800 rounded-md pl-10 pr-4 py-2.5 text-sm focus:outline-none focus:ring-1 focus:ring-theme-cyan/30 focus:border-theme-cyan/30"
                        />
                    </div>
                    
                    <div className="flex items-center justify-between bg-zinc-900/50 p-3 rounded-lg border border-zinc-800">
                        <div className="flex items-center gap-6">
                            <button 
                                onClick={handleToggleAll}
                                className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-zinc-400 hover:text-white transition-colors"
                            >
                                {selectedIds.size === entries.length ? (
                                    <CheckSquare className="h-4 w-4 text-theme-cyan" />
                                ) : (
                                    <Square className="h-4 w-4" />
                                )}
                                {selectedIds.size === entries.length ? "Deselect All" : "Select All"}
                            </button>
                            
                            <div className="flex items-center gap-2 text-xs text-zinc-500 font-mono">
                                <ListFilter className="h-3 w-3" />
                                <span>{selectedIds.size} / {entries.length} selected</span>
                            </div>
                        </div>

                        <div className="flex items-center gap-2">
                             <Button variant="ghost" size="sm" onClick={onClose} className="h-8">
                                Cancel
                             </Button>
                             <Button 
                                variant="neon" 
                                size="sm" 
                                className="h-8 font-black"
                                onClick={handleConfirm}
                                disabled={selectedIds.size === 0}
                             >
                                <Download className="h-3 w-3 mr-2" />
                                Queue {selectedIds.size} Items
                             </Button>
                        </div>
                    </div>
                </div>

                {/* List Container */}
                <div className="flex-1 overflow-y-auto pr-2 custom-scrollbar">
                    <div className="space-y-1">
                        {filteredEntries.map((entry, index) => {
                            const isSelected = selectedIds.has(entry.url);
                            return (
                                <div 
                                    key={entry.url}
                                    onClick={() => handleToggleEntry(entry.url)}
                                    className={twMerge(
                                        "flex items-center gap-4 p-3 rounded-md border cursor-pointer transition-all",
                                        isSelected 
                                            ? "bg-theme-cyan/5 border-theme-cyan/20" 
                                            : "bg-zinc-900/30 border-transparent hover:border-zinc-800 hover:bg-zinc-900/60"
                                    )}
                                >
                                    <div className="flex-shrink-0 font-mono text-[10px] text-zinc-600 w-6">
                                        {(index + 1).toString().padStart(2, '0')}
                                    </div>
                                    <div className="flex-1 min-w-0">
                                        <div className={twMerge(
                                            "text-sm font-medium truncate",
                                            isSelected ? "text-zinc-100" : "text-zinc-400"
                                        )}>
                                            {entry.title}
                                        </div>
                                        <div className="text-[10px] text-zinc-600 truncate font-mono mt-0.5">
                                            {entry.url}
                                        </div>
                                    </div>
                                    <div className={twMerge(
                                        "w-5 h-5 rounded border flex items-center justify-center transition-colors",
                                        isSelected ? "bg-theme-cyan border-theme-cyan text-black" : "border-zinc-700 bg-black/20"
                                    )}>
                                        {isSelected && <CheckSquare className="h-3.5 w-3.5" />}
                                    </div>
                                </div>
                            );
                        })}
                        {filteredEntries.length === 0 && (
                            <div className="py-12 text-center text-zinc-600">
                                <X className="h-8 w-8 mx-auto mb-2 opacity-20" />
                                <p className="text-sm">No results match your search.</p>
                            </div>
                        )}
                    </div>
                </div>
            </div>
        </Modal>
    );
}