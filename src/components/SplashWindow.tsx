import { useState, useEffect, useRef } from 'react';
import { syncDependencies, closeSplash, installDependency, openExternalLink } from '@/api/invoke';
import { listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import icon from '@/assets/icon.webp';
import { RefreshCw, ShieldCheck, Terminal, Download, XCircle, Loader2, ExternalLink } from 'lucide-react';
import { Progress } from './ui/Progress';
import { Button } from './ui/Button';
import { AppDependencies } from '@/types';

interface InstallProgress {
    name: string;
    percentage: number;
    status: string;
}

type SplashStatus = 'init' | 'syncing' | 'js-analysis' | 'ready' | 'error';

export function SplashWindow() {
  const [status, setStatus] = useState<SplashStatus>('init');
  const [message, setMessage] = useState('Initializing Core...');
  const [installState, setInstallState] = useState<InstallProgress>({ name: '', percentage: 0, status: '' });
  const [appVersion, setAppVersion] = useState('');
  const [errorDetails, setErrorDetails] = useState('');
  const [deps, setDeps] = useState<AppDependencies | null>(null);
  
  const hasRun = useRef(false);

  const performSync = async () => {
    try {
        setStatus('syncing');
        setMessage('Verifying System Integrity...');
        
        // syncDependencies is now optimized on the backend to avoid redundant network calls
        const finalDeps = await syncDependencies();
        setDeps(finalDeps);

        const js = finalDeps.js_runtime;
        
        // Critical block: If JS is missing, we MUST ask the user
        if (!js.available || !js.is_supported) {
            setStatus('js-analysis');
            setMessage('Action Required: Runtime Missing');
            return;
        }

        // Non-critical: If JS is legacy but working, we just launch and let them update later
        finishStartup();
    } catch (e) {
        console.error("Sync Error:", e);
        setStatus('error');
        setErrorDetails(`${e}`);
        setMessage('Critical synchronization failure.');
    }
  };

  const finishStartup = async () => {
      setStatus('ready');
      setMessage('System Optimal. Launching...');
      // Minimal artificial delay for smooth transition
      setTimeout(async () => { 
          try {
              await closeSplash(); 
          } catch (err) {
              console.error("Failed to transition window", err);
              setErrorDetails(`${err}`);
              setStatus('error');
          }
      }, 400); 
  };

  const handleInstallDeno = async () => {
      setStatus('syncing');
      setMessage('Installing Deno (Recommended)...');
      try {
          await installDependency('deno');
          await performSync();
      } catch (e) {
          setStatus('error');
          setErrorDetails(`${e}`);
      }
  };

  const handleUpdateCurrent = async () => {
      if (!deps?.js_runtime.name) return;
      const runtimeName = deps.js_runtime.name.toLowerCase();

      if (runtimeName === 'node') {
          await openExternalLink("https://nodejs.org/en/download");
          return;
      }

      setStatus('syncing');
      setMessage(`Updating ${deps.js_runtime.name}...`);
      try {
          await installDependency(deps.js_runtime.name);
          await performSync();
      } catch (e) {
          setStatus('error');
          setErrorDetails(`${e}`);
      }
  };

  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`));
    
    const unlisten = listen<InstallProgress>('install-progress', (event) => {
        setInstallState(event.payload);
        if (event.payload.status) setMessage(event.payload.status);
    });

    if (!hasRun.current) {
        hasRun.current = true;
        // Start sync immediately
        performSync();
    }

    return () => { unlisten.then(f => f()); };
  }, []);

  return (
    <div className="h-screen w-screen bg-zinc-950 flex flex-col items-center justify-center relative overflow-hidden border-2 border-zinc-900 cursor-default select-none">
      <div className="absolute inset-0 bg-[linear-gradient(rgba(18,18,18,0)_1px,transparent_1px),linear-gradient(90deg,rgba(18,18,18,0)_1px,transparent_1px)] bg-[size:40px_40px] [mask-image:radial-gradient(ellipse_80%_50%_at_50%_50%,black,transparent)] pointer-events-none" />

      <div className="z-20 flex flex-col items-center w-full max-w-[340px]">
        <div className="glitch-wrapper mb-8">
            <div className="glitch-logo" style={{ backgroundImage: `url(${icon})` }} />
        </div>

        <div className="text-center space-y-2 mb-6">
            <h1 className={`font-mono font-bold text-lg tracking-wider uppercase transition-colors duration-300 ${
                status === 'error' ? 'text-theme-red' : 'text-theme-cyan'
            }`}>
                {status === 'init' && 'Initializing'}
                {status === 'syncing' && 'Syncing'}
                {status === 'js-analysis' && 'Requirement Check'}
                {status === 'ready' && 'Ready'}
                {status === 'error' && 'Sync Failed'}
            </h1>
            <p className="text-zinc-500 text-xs font-medium min-h-[16px] px-4">{message}</p>
        </div>

        {status === 'js-analysis' && deps && (
            <div className="w-full space-y-4 animate-fade-in bg-zinc-900/80 p-5 rounded-lg border border-zinc-800 backdrop-blur-md">
                <div className="flex items-center gap-3 text-amber-500">
                    <Terminal className="h-5 w-5" />
                    <span className="text-sm font-bold uppercase">JS Runtime Notice</span>
                </div>
                
                <p className="text-[11px] text-zinc-400 leading-relaxed">
                    {!deps.js_runtime.available || !deps.js_runtime.is_supported 
                        ? "No supported JS runtime was detected. YouTube extraction relies on modern JS engines. We highly recommend installing Deno."
                        : `Your current runtime (${deps.js_runtime.name}) is supported, but a newer build is recommended for stability.`}
                </p>

                <div className="space-y-2">
                    <Button size="sm" variant="neon" className="w-full h-9 text-xs" onClick={handleInstallDeno}>
                        <Download className="h-3 w-3 mr-2" /> Install Deno
                    </Button>
                    
                    {deps.js_runtime.available && (
                         <Button size="sm" variant="outline" className="w-full h-9 text-xs" onClick={handleUpdateCurrent}>
                            {deps.js_runtime.name.toLowerCase() === 'node' ? (
                                <>
                                    <ExternalLink className="h-3 w-3 mr-2" /> Get Node JS
                                </>
                            ) : (
                                <>
                                    <RefreshCw className="h-3 w-3 mr-2" /> Update Current
                                </>
                            )}
                         </Button>
                    )}

                    <Button size="sm" variant="ghost" className="w-full h-9 text-xs text-zinc-500" onClick={finishStartup}>
                        <XCircle className="h-3 w-3 mr-2" /> Dismiss & Launch
                    </Button>
                </div>
            </div>
        )}

        {(status === 'syncing' || status === 'init') && (
            <div className="w-full space-y-3 animate-fade-in bg-black/50 p-4 rounded-lg border border-zinc-800">
                <div className="flex items-center justify-between text-xs text-zinc-300">
                    <div className="flex items-center gap-2">
                        <Loader2 className="h-3 w-3 animate-spin text-theme-cyan" />
                        <span className="font-bold uppercase">{installState.name || 'Resources'}</span>
                    </div>
                    <span className="font-mono text-theme-cyan">{installState.percentage || 0}%</span>
                </div>
                <Progress value={installState.percentage || 0} className="h-1" />
            </div>
        )}

        {status === 'ready' && (
             <div className="flex items-center gap-2 text-emerald-500 animate-fade-in bg-emerald-500/10 px-4 py-2 rounded-full border border-emerald-500/20">
                <ShieldCheck className="h-4 w-4" />
                <span className="text-xs font-bold uppercase tracking-wider">Verified</span>
             </div>
        )}

        {status === 'error' && (
            <div className="w-full space-y-3 animate-fade-in bg-black/50 p-4 rounded-lg border border-theme-red/30">
                <div className="text-[10px] text-zinc-400 font-mono bg-zinc-900 p-2 rounded border border-zinc-800 break-all max-h-24 overflow-y-auto">
                    {errorDetails || "Unknown synchronization error. Please check internet and logs."}
                </div>
                <div className="flex gap-2">
                    <Button size="sm" className="flex-1" variant="secondary" onClick={() => { performSync(); }}>Retry</Button>
                    <Button size="sm" className="flex-1" variant="ghost" onClick={finishStartup}>Skip</Button>
                </div>
            </div>
        )}
      </div>
      
      <div className="absolute bottom-4 text-[10px] text-zinc-700 font-mono">
         {appVersion || 'Checking version...'}
      </div>
    </div>
  );
}