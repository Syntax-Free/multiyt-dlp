import { useAppContext } from '@/contexts/AppContext';
import { FileKey, FolderOpen, Lock, ListChecks, X } from 'lucide-react';
import { open } from '@tauri-apps/api/dialog';
import { Button } from '../ui/Button';
import { TemplateEditor } from './TemplateEditor';

const BROWSERS = [
    { label: 'None', value: 'none' },
    { label: 'Chrome', value: 'chrome' },
    { label: 'Firefox', value: 'firefox' },
    { label: 'Edge', value: 'edge' },
    { label: 'Brave', value: 'brave' },
    { label: 'Opera', value: 'opera' },
    { label: 'Vivaldi', value: 'vivaldi' },
    { label: 'Safari', value: 'safari' },
];

export function YtdlpSettings() {
    const { 
        cookiesPath,
        setCookiesPath,
        cookiesBrowser,
        setCookiesBrowser,
        filenameTemplateBlocks,
        setFilenameTemplateBlocks,
        preferences,
        updatePreferences
    } = useAppContext();

    const handleSelectCookieFile = async () => {
        try {
            const selected = await open({
                multiple: false,
                filters: [{ name: 'Text Files', extensions: ['txt'] }]
            });
            if (selected && typeof selected === 'string') {
                setCookiesPath(selected);
            }
        } catch (err) {
            console.error("Failed to select cookie file", err);
        }
    };

    return (
        <div className="space-y-10 animate-fade-in pb-12">
            
            {/* Playlist Behavior */}
            <div id="section-playlist" className="space-y-4 scroll-mt-6">
                 <div>
                    <h3 className="text-base font-medium text-zinc-100">Playlist Behavior</h3>
                    <p className="text-sm text-zinc-500">
                        Configure how the app handles links containing multiple videos.
                    </p>
                </div>
                <hr className="border-zinc-800" />

                <div className="bg-zinc-900/30 p-5 rounded-lg border border-zinc-800/50">
                    <button 
                        onClick={() => updatePreferences({ enable_playlist_selection: !preferences.enable_playlist_selection })}
                        className="flex items-center justify-between w-full group"
                    >
                        <div className="flex items-center gap-4">
                            <div className={`p-2 rounded-md transition-colors ${preferences.enable_playlist_selection ? 'bg-theme-cyan/10 text-theme-cyan' : 'bg-zinc-800 text-zinc-500'}`}>
                                <ListChecks className="h-5 w-5" />
                            </div>
                            <div className="text-left">
                                <div className="text-sm font-medium text-zinc-200 group-hover:text-white transition-colors">Manual Playlist Selection</div>
                                <div className="text-xs text-zinc-500">Show a selection screen for playlists and channels.</div>
                            </div>
                        </div>
                        <div className={`w-10 h-5 rounded-full relative transition-colors ${preferences.enable_playlist_selection ? 'bg-theme-cyan' : 'bg-zinc-700'}`}>
                            <div className={`absolute top-1 w-3 h-3 bg-white rounded-full transition-all ${preferences.enable_playlist_selection ? 'left-6' : 'left-1'}`} />
                        </div>
                    </button>
                </div>
            </div>

            {/* Filename Formatting */}
            <div id="section-formatting" className="space-y-4 scroll-mt-6">
                 <div>
                    <h3 className="text-base font-medium text-zinc-100">Filename Formatting</h3>
                    <p className="text-sm text-zinc-500">
                        Drag and drop blocks to customize how your downloaded files are named.
                    </p>
                </div>
                <hr className="border-zinc-800" />
                
                <TemplateEditor 
                    blocks={filenameTemplateBlocks} 
                    onChange={setFilenameTemplateBlocks} 
                />
            </div>

            {/* Cookies & Auth Section */}
            <div id="section-cookies" className="space-y-4 scroll-mt-6">
                <div>
                    <h3 className="text-base font-medium text-zinc-100">Cookies & Authentication</h3>
                    <p className="text-sm text-zinc-500">
                        Load cookies to access age-restricted content or premium subscriptions.
                    </p>
                </div>
                <hr className="border-zinc-800" />
                
                <div className="bg-zinc-900/30 p-5 rounded-lg border border-zinc-800/50 grid gap-6">
                    {/* File Method */}
                    <div className="space-y-2">
                        <div className="flex justify-between items-center">
                            <label className="text-sm font-medium text-zinc-300 flex items-center gap-2">
                                <FileKey className="h-4 w-4 text-theme-cyan" />
                                Load from File (cookies.txt)
                            </label>
                            {cookiesPath && (
                                <button onClick={() => setCookiesPath(null)} className="text-xs text-red-400 hover:text-red-300 flex items-center gap-1 transition-colors">
                                    <X className="h-3 w-3" /> Clear
                                </button>
                            )}
                        </div>
                        <div className="flex gap-2">
                             <input
                                type="text"
                                value={cookiesPath || ''}
                                readOnly
                                placeholder="No cookie file selected"
                                className={`flex-grow bg-zinc-900 border rounded-md px-3 py-2 text-sm text-zinc-400 focus:outline-none transition-colors ${cookiesPath ? 'border-theme-cyan/50 text-zinc-200' : 'border-zinc-800'}`}
                             />
                             <Button 
                                type="button" 
                                variant="secondary" 
                                onClick={handleSelectCookieFile} 
                                className="border-zinc-700 hover:border-zinc-500 hover:text-white"
                             >
                                <FolderOpen className="h-4 w-4" />
                             </Button>
                        </div>
                    </div>
                    
                    <div className="relative flex py-1 items-center">
                        <div className="flex-grow border-t border-zinc-800"></div>
                        <span className="flex-shrink-0 mx-4 text-[10px] text-zinc-600 uppercase font-bold tracking-wider">OR</span>
                        <div className="flex-grow border-t border-zinc-800"></div>
                    </div>

                    {/* Browser Method */}
                    <div className="space-y-2">
                         <label className="text-sm font-medium text-zinc-300 flex items-center gap-2">
                            <Lock className="h-4 w-4 text-amber-500" />
                            Extract from Browser
                         </label>
                         <select 
                             value={cookiesBrowser || 'none'}
                             onChange={(e) => setCookiesBrowser(e.target.value)}
                             className={`w-full bg-zinc-900 border rounded-md px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:ring-1 transition-all ${cookiesBrowser && cookiesBrowser !== 'none' ? 'border-amber-500/50 focus:ring-amber-500/50' : 'border-zinc-800 focus:ring-theme-cyan/50'}`}
                         >
                             {BROWSERS.map(b => (
                                 <option key={b.value} value={b.value}>{b.label}</option>
                             ))}
                         </select>
                         <p className="text-xs text-zinc-500">
                            Uses <code>--cookies-from-browser</code>. Ensure the browser is closed before starting downloads if you experience issues.
                         </p>
                    </div>
                </div>
            </div>
        </div>
    );
}