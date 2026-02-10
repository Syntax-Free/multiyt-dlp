import { useState, useEffect, useRef } from 'react';
import { checkLocalDeps, checkYtdlpUpdate, closeSplash, installDependency, getAppConfig, requestAttention } from '@/api/invoke';
import { listen } from '@tauri-apps/api/event';
import icon from '@/assets/icon.webp';
import { Loader2, Zap, ZapOff, PlayCircle } from 'lucide-react';
import { Progress } from './ui/Progress';
import { Button } from './ui/Button';

interface InstallProgress {
    name: string;
    percentage: number;
    status: string;
}

type SplashStatus = 'init' | 'check-updates' | 'aria-prompt' | 'installing' | 'ready' | 'error';

export function SplashWindow() {
  const [status, setStatus] = useState<SplashStatus>('init');
  const [message, setMessage] = useState('Checking system...');
  const [installState, setInstallState] = useState<InstallProgress>({ name: '', percentage: 0, status: '' });
  const [errorDetails, setErrorDetails] = useState('');
  
  // State for the "Skip" button timeout logic
  const [showSkip, setShowSkip] = useState(false);
  const skipTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  
  const hasRun = useRef(false);
  const pendingInstalls = useRef<string[]>([]);

  // --- Core State Machine ---

  const bootSequence = async () => {
    try {
        setStatus('init');
        setMessage('Verifying Integrity...');

        // 1. Instant Local Scan
        const scan = await checkLocalDeps();
        
        // Add missing local binaries to pending
        scan.missing.forEach(dep => {
            if (!pendingInstalls.current.includes(dep)) {
                pendingInstalls.current.push(dep);
            }
        });

        // 2. Network Check for yt-dlp (Only if it exists locally)
        if (!scan.missing.includes('yt-dlp')) {
            setStatus('check-updates');
            setMessage('Checking for yt-dlp updates...');
            
            // Start the "Skip" timer
            setShowSkip(false);
            skipTimerRef.current = setTimeout(() => {
                setShowSkip(true);
            }, 5000); // 5 Seconds hard limit for UI feedback

            try {
                const needsUpdate = await checkYtdlpUpdate();
                if (needsUpdate) {
                    if (!pendingInstalls.current.includes('yt-dlp')) {
                        pendingInstalls.current.push('yt-dlp');
                    }
                }
            } catch (err) {
                console.warn("Update check failed or skipped", err);
            } finally {
                if (skipTimerRef.current) clearTimeout(skipTimerRef.current);
                setShowSkip(false);
            }
        }

        // 3. Lazy Aria2 Check
        // Only trigger if we have heavy lifting to do
        const config = await getAppConfig();
        if (pendingInstalls.current.length > 0) {
            if (!scan.aria2_available && !config.general.aria2_prompt_dismissed) {
                setStatus('aria-prompt');
                setMessage('Optimize Download Speed?');
                requestAttention();
                return; // Wait for user interaction
            }
        }

        // 4. Proceed to Installs or Launch
        processInstalls();

    } catch (e) {
        console.error("Boot Error:", e);
        setStatus('error');
        setErrorDetails(`${e}`);
        setMessage('Initialization Failed.');
    }
  };

  const processInstalls = async () => {
      if (pendingInstalls.current.length === 0) {
          finishStartup();
          return;
      }

      setStatus('installing');
      
      try {
          // Process sequentially
          for (const dep of pendingInstalls.current) {
              setMessage(`Installing ${dep}...`);
              // Reset progress bar for visual clarity
              setInstallState({ name: dep, percentage: 0, status: 'Starting...' });
              await installDependency(dep);
          }
          finishStartup();
      } catch (e) {
          setStatus('error');
          setErrorDetails(`${e}`);
      }
  };

  const handleAriaChoice = async (install: boolean) => {
    if (install) {
        // Prepend Aria2 to the list so it installs first
        pendingInstalls.current.unshift('aria2');
    }
    processInstalls();
  };

  const handleSkipUpdate = () => {
      // Forcefully move to next step, ignoring the running promise
      if (skipTimerRef.current) clearTimeout(skipTimerRef.current);
      setShowSkip(false);
      
      // We assume checking failed or was too slow, so we don't add yt-dlp to updates
      // Proceed to logic step 3
      checkLocalDeps().then(async (scan) => {
           const config = await getAppConfig();
           if (pendingInstalls.current.length > 0) {
                if (!scan.aria2_available && !config.general.aria2_prompt_dismissed) {
                    setStatus('aria-prompt');
                    setMessage('Optimize Download Speed?');
                    return;
                }
           }
           processInstalls();
      });
  };

  const finishStartup = async () => {
      setStatus('ready');
      setMessage('Launching...');
      setTimeout(async () => { 
          try {
              await closeSplash(); 
          } catch (err) {
              console.error("Failed to transition window", err);
              setErrorDetails(`${err}`);
              setStatus('error');
          }
      }, 300); 
  };

  // --- Effects ---

  useEffect(() => {
    const unlisten = listen<InstallProgress>('install-progress', (event) => {
        setInstallState(event.payload);
        if (event.payload.status) setMessage(event.payload.status);
    });

    if (!hasRun.current) {
        hasRun.current = true;
        bootSequence();
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
                {status === 'init' && 'System Check'}
                {status === 'check-updates' && 'Checking Updates'}
                {status === 'aria-prompt' && 'Accelerator'}
                {status === 'installing' && 'Updating'}
                {status === 'ready' && 'Ready'}
                {status === 'error' && 'Sync Failed'}
            </h1>
            <p className="text-zinc-500 text-xs font-medium min-h-[16px] px-4 animate-fade-in">
                {message}
            </p>
        </div>

        {/* --- SKIP UPDATE BUTTON --- */}
        {status === 'check-updates' && showSkip && (
            <div className="animate-fade-in w-full px-10">
                 <Button 
                    size="sm" 
                    variant="outline" 
                    className="w-full h-8 text-[10px] border-zinc-800 text-zinc-500 hover:text-zinc-300" 
                    onClick={handleSkipUpdate}
                >
                    <PlayCircle className="h-3 w-3 mr-2" /> Skip Check (Taking too long)
                </Button>
            </div>
        )}

        {/* --- ARIA PROMPT --- */}
        {status === 'aria-prompt' && (
            <div className="w-full space-y-4 animate-fade-in bg-zinc-900/80 p-5 rounded-lg border border-zinc-800 backdrop-blur-md">
                <div className="flex items-center gap-3 text-theme-cyan">
                    <Zap className="h-5 w-5" />
                    <span className="text-sm font-bold uppercase">Turbocharge?</span>
                </div>
                
                <p className="text-[11px] text-zinc-400 leading-relaxed">
                    We detected missing components. Aria2 can speed up the installation significantly using multi-threaded downloads.
                </p>

                <div className="space-y-2">
                    <Button size="sm" variant="neon" className="w-full h-8 text-xs" onClick={() => handleAriaChoice(true)}>
                        <Zap className="h-3 w-3 mr-2" /> Yes, Install Aria2
                    </Button>
                    
                    <Button size="sm" variant="outline" className="w-full h-8 text-xs" onClick={() => handleAriaChoice(false)}>
                        <ZapOff className="h-3 w-3 mr-2" /> No, Use Native
                    </Button>
                </div>
            </div>
        )}

        {/* --- PROGRESS BAR --- */}
        {status === 'installing' && (
            <div className="w-full space-y-3 animate-fade-in bg-black/50 p-4 rounded-lg border border-zinc-800">
                <div className="flex items-center justify-between text-xs text-zinc-300">
                    <div className="flex items-center gap-2">
                        <Loader2 className="h-3 w-3 animate-spin text-theme-cyan" />
                        <span className="font-bold uppercase">{installState.name || 'Processing'}</span>
                    </div>
                    <span className="font-mono text-theme-cyan">{installState.percentage || 0}%</span>
                </div>
                <Progress value={installState.percentage || 0} className="h-1" />
            </div>
        )}

        {/* --- ERROR STATE --- */}
        {status === 'error' && (
            <div className="w-full space-y-3 animate-fade-in bg-black/50 p-4 rounded-lg border border-theme-red/30">
                <div className="text-[10px] text-zinc-400 font-mono bg-zinc-900 p-2 rounded border border-zinc-800 break-all max-h-24 overflow-y-auto">
                    {errorDetails || "Unknown synchronization error. Please check internet and logs."}
                </div>
                <div className="flex gap-2">
                    <Button size="sm" className="flex-1" variant="secondary" onClick={() => { bootSequence(); }}>Retry</Button>
                    <Button size="sm" className="flex-1" variant="ghost" onClick={finishStartup}>Skip</Button>
                </div>
            </div>
        )}
      </div>
      
      <div className="absolute bottom-4 flex flex-col items-center gap-1">
         <div className="w-1 h-1 rounded-full bg-zinc-800 animate-pulse"></div>
      </div>
    </div>
  );
}