import { ReactNode } from 'react';
import { Settings, AlertTriangle } from 'lucide-react';
import { Button } from './ui/Button';
import { SettingsModal } from './settings/SettingsModal';
import { useAppContext } from '@/contexts/AppContext';
import { Toast } from './ui/Toast';
import { SFIcon } from './icons/SFIcon';

interface LayoutProps {
  SidebarContent: ReactNode;
  MainContent: ReactNode;
}

export function Layout({ SidebarContent, MainContent }: LayoutProps) {
  const { isJsRuntimeMissing, openSettings } = useAppContext();

  return (
    <div className="flex h-screen overflow-hidden bg-zinc-900 text-zinc-100">
      <SettingsModal />
      
      {/* Toast Notification Layer */}
      <Toast />
      
      {/* Sidebar */}
      <aside className="w-80 flex-shrink-0 bg-zinc-900/50 border-r border-zinc-800 p-4 overflow-y-auto flex flex-col">
        <div className="flex items-center justify-between px-2 mb-8 mt-4 group">
            <div className="flex items-center gap-3">
                {/* Icon with Scale Animation Only */}
                <SFIcon className="w-10 h-10 transition-transform duration-300 group-hover:scale-110" />
                
                <div>
                    <h1 className="text-lg font-bold tracking-tight text-white leading-none">
                        Multiyt-dlp
                    </h1>
                    {/* Reverted Text */}
                    <div className="text-xs text-zinc-500 mt-1">
                        SYN SQUAD
                    </div>
                </div>
            </div>
            <Button 
                variant="ghost" 
                size="icon" 
                title="Settings" 
                className="text-zinc-500 hover:text-white"
                onClick={() => openSettings('general')}
            >
                <Settings className="h-5 w-5" />
            </Button>
        </div>

        {isJsRuntimeMissing && (
            <div className="mb-6 px-3 py-3 bg-amber-950/20 border border-amber-500/20 rounded-lg text-amber-500 flex gap-3">
                <AlertTriangle className="h-5 w-5 flex-shrink-0" />
                <div className="text-xs leading-relaxed">
                    <span className="font-bold block mb-1">Limited Functionality</span>
                    No JavaScript runtime detected (Node, Deno, or Bun). YouTube downloads may fail or be restricted.
                </div>
            </div>
        )}

        {SidebarContent}
      </aside>
      
      {/* Main Content */}
      <main className="flex-grow p-8 overflow-y-auto bg-zinc-950">
        <div className="max-w-4xl mx-auto">
            {MainContent}
        </div>
      </main>
    </div>
  );
}