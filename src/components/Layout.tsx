import { ReactNode } from 'react';
import { Settings, AlertTriangle } from 'lucide-react';
import { Button } from './ui/Button';
import { SettingsModal } from './settings/SettingsModal';
import { useAppContext } from '@/contexts/AppContext';
import { Toast } from './ui/Toast';
import { UpdateChecker } from './UpdateChecker';
import icon from '@/assets/icon.webp';

interface LayoutProps {
  SidebarContent: ReactNode;
  MainContent: ReactNode;
}

export function Layout({ SidebarContent, MainContent }: LayoutProps) {
  const { isJsRuntimeMissing, openSettings } = useAppContext();

  return (
    <div className="flex h-screen overflow-hidden bg-zinc-900 text-zinc-100 relative">
      <SettingsModal />
      
      {/* Notifications */}
      <Toast />
      <UpdateChecker />
      
      {/* Sidebar */}
      <aside className="w-80 flex-shrink-0 bg-zinc-900/50 border-r border-zinc-800 p-4 overflow-y-auto flex flex-col">
        <div className="flex items-center justify-between px-2 mb-8 mt-4 group">
            <div className="flex items-center gap-3">
                {/* Official App Icon */}
                <img 
                    src={icon} 
                    alt="App Icon" 
                    className="w-10 h-10 transition-transform duration-300 group-hover:scale-110 shadow-glow-cyan rounded-lg"
                />
                
                <div>
                    <h1 className="text-lg font-bold tracking-tight text-white leading-none">
                        Multiyt-dlp
                    </h1>
                    <div className="text-xs text-zinc-500 mt-1">
                        SYN SQUAD
                    </div>
                </div>
            </div>
            <Button 
                variant="ghost" 
                size="icon" 
                title="Settings" 
                className="group text-zinc-500 hover:text-white"
                onClick={() => openSettings('general')}
            >
                <Settings className="h-5 w-5 transition-all duration-500 group-hover:animate-[spin_3s_linear_infinite]" />
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