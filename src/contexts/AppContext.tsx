import React, { useState, useEffect, useCallback, useRef } from 'react';
import { TemplateBlock, PreferenceConfig } from '@/types';
import { getAppConfig, saveGeneralConfig, savePreferenceConfig, checkDependencies, getLatestAppVersion } from '@/api/invoke';
import { getVersion } from '@tauri-apps/api/app';

interface AppContextType {
  // State
  isConfigLoaded: boolean;
  isJsRuntimeMissing: boolean;
  setIsJsRuntimeMissing: (missing: boolean) => void;

  // Settings Modal State (Global)
  isSettingsOpen: boolean;
  settingsActiveTab: string;
  settingsActiveSection: string | null;
  openSettings: (tab?: string, sectionId?: string) => void;
  closeSettings: () => void;
  setSettingsActiveTab: (tab: string) => void;

  // General Config
  defaultDownloadPath: string | null;
  setDefaultDownloadPath: (path: string) => void;
  filenameTemplateBlocks: TemplateBlock[];
  setFilenameTemplateBlocks: (blocks: TemplateBlock[]) => void;
  getTemplateString: (blocks?: TemplateBlock[]) => string;
  
  // Cookies Config
  cookiesPath: string | null;
  setCookiesPath: (path: string | null) => void;
  cookiesBrowser: string | null;
  setCookiesBrowser: (browser: string | null) => void;
  
  // Concurrency
  maxConcurrentDownloads: number;
  maxTotalInstances: number;
  setConcurrency: (concurrent: number, total: number) => void;

  // Logs
  logLevel: string;
  setLogLevel: (level: string) => void;

  // Update
  checkForUpdates: boolean;
  setCheckForUpdates: (enabled: boolean) => void;
  isUpdateAvailable: boolean;
  latestVersion: string | null;
  currentVersion: string | null;
  checkAppUpdate: () => Promise<void>;

  // Preferences
  preferences: PreferenceConfig;
  updatePreferences: (updates: Partial<PreferenceConfig>) => void;
}

const DEFAULT_TEMPLATE_BLOCKS: TemplateBlock[] = [
  { id: 'def-1', type: 'variable', value: 'title', label: 'Title' },
  { id: 'def-2', type: 'separator', value: '.', label: '.' },
  { id: 'def-3', type: 'variable', value: 'ext', label: 'Extension' },
];

const DEFAULT_PREFS: PreferenceConfig = {
    mode: 'video',
    format_preset: 'best',
    video_preset: 'best',        
    audio_preset: 'audio_best',  
    video_resolution: 'best',
    embed_metadata: false,
    embed_thumbnail: false,
    live_from_start: false
};

export const AppContext = React.createContext<AppContextType | undefined>(undefined);

export const AppProvider = ({ children }: { children: React.ReactNode }) => {
  const [isConfigLoaded, setIsConfigLoaded] = useState(false);
  const [isJsRuntimeMissing, setIsJsRuntimeMissing] = useState(false);

  // Settings Modal State
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsActiveTab, setSettingsActiveTab] = useState('general');
  const [settingsActiveSection, setSettingsActiveSection] = useState<string | null>(null);

  // Config State
  const [defaultDownloadPath, _setDownloadPath] = useState<string | null>(null);
  const [filenameTemplateBlocks, _setTemplateBlocks] = useState<TemplateBlock[]>(DEFAULT_TEMPLATE_BLOCKS);
  const [preferences, _setPreferences] = useState<PreferenceConfig>(DEFAULT_PREFS);
  
  // Cookie State
  const [cookiesPath, _setCookiesPath] = useState<string | null>(null);
  const [cookiesBrowser, _setCookiesBrowser] = useState<string | null>(null);

  // Concurrency State
  const [maxConcurrentDownloads, _setMaxConcurrentDownloads] = useState(4);
  const [maxTotalInstances, _setMaxTotalInstances] = useState(10);
  
  // Log State
  const [logLevel, _setLogLevel] = useState('info');

  // Update State
  const [checkForUpdates, _setCheckForUpdates] = useState(true);
  const [isUpdateAvailable, setIsUpdateAvailable] = useState(false);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);

  // DEFECT FIX #2: Debouncing for config save
  // We use a ref to store the latest config state, and a useEffect to trigger save with delay.
  // This prevents UI lag and file locking issues on the backend.
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // --- Settings Navigation Logic ---
  const openSettings = useCallback((tab?: string, sectionId?: string) => {
    if (tab) setSettingsActiveTab(tab);
    if (sectionId) setSettingsActiveSection(sectionId);
    else setSettingsActiveSection(null);
    setIsSettingsOpen(true);
  }, []);

  const closeSettings = useCallback(() => {
    setIsSettingsOpen(false);
    // Reset section but keep tab
    setSettingsActiveSection(null);
  }, []);

  const checkAppUpdate = async () => {
    try {
        const current = await getVersion();
        setCurrentVersion(current);
        const latestTag = await getLatestAppVersion();
        
        const cleanLatest = latestTag.replace(/^v/, '');
        const cleanCurrent = current.replace(/^v/, '');

        setLatestVersion(cleanLatest);

        const v1parts = cleanCurrent.split('.').map(Number);
        const v2parts = cleanLatest.split('.').map(Number);
        
        let isNewer = false;
        for (let i = 0; i < Math.max(v1parts.length, v2parts.length); i++) {
            const v1 = v1parts[i] || 0;
            const v2 = v2parts[i] || 0;
            if (v2 > v1) { isNewer = true; break; }
            if (v1 > v2) { break; }
        }

        setIsUpdateAvailable(isNewer);
    } catch (e) {
        console.warn("Update check failed:", e);
    }
  };

  useEffect(() => {
    const load = async () => {
      try {
        const config = await getAppConfig();
        
        if (config.general.download_path) _setDownloadPath(config.general.download_path);
        if (config.general.cookies_path) _setCookiesPath(config.general.cookies_path);
        if (config.general.cookies_from_browser) _setCookiesBrowser(config.general.cookies_from_browser);

        _setMaxConcurrentDownloads(config.general.max_concurrent_downloads);
        _setMaxTotalInstances(config.general.max_total_instances);
        _setLogLevel(config.general.log_level || 'info');
        _setCheckForUpdates(config.general.check_for_updates);

        if (config.general.template_blocks_json) {
            try {
                const parsed = JSON.parse(config.general.template_blocks_json);
                _setTemplateBlocks(parsed);
            } catch(e) { console.warn("Failed to parse blocks", e); }
        }

        _setPreferences({ ...DEFAULT_PREFS, ...config.preferences });
        
        const deps = await checkDependencies();
        if (!deps.js_runtime.available) {
            setIsJsRuntimeMissing(true);
        }

        if (config.general.check_for_updates) {
            checkAppUpdate();
        } else {
            getVersion().then(v => setCurrentVersion(v));
        }

      } catch (error) {
        console.error("Failed to load config:", error);
      } finally {
        setIsConfigLoaded(true);
      }
    };
    load();
  }, []);

  const getTemplateString = useCallback((blocks?: TemplateBlock[]) => {
    const target = blocks || filenameTemplateBlocks;
    return target.map(block => {
        if (block.type === 'variable') {
            return `%(${block.value})s`;
        }
        return block.value;
    }).join('');
  }, [filenameTemplateBlocks]);

  // Debounced Save Trigger
  const triggerDebouncedSave = useCallback(() => {
      if (saveTimeoutRef.current) {
          clearTimeout(saveTimeoutRef.current);
      }

      saveTimeoutRef.current = setTimeout(() => {
          saveGeneralConfig({
            download_path: defaultDownloadPath,
            filename_template: getTemplateString(filenameTemplateBlocks),
            template_blocks_json: JSON.stringify(filenameTemplateBlocks),
            max_concurrent_downloads: maxConcurrentDownloads,
            max_total_instances: maxTotalInstances,
            log_level: logLevel,
            check_for_updates: checkForUpdates,
            cookies_path: cookiesPath,
            cookies_from_browser: cookiesBrowser
          }).catch(e => console.error("Failed to save general config:", e));
      }, 500); // 500ms debounce
  }, [
      defaultDownloadPath, 
      filenameTemplateBlocks, 
      maxConcurrentDownloads, 
      maxTotalInstances, 
      logLevel, 
      checkForUpdates, 
      cookiesPath, 
      cookiesBrowser,
      getTemplateString
  ]);

  // Whenever dependencies change, schedule a save
  useEffect(() => {
      if (isConfigLoaded) {
          triggerDebouncedSave();
      }
  }, [triggerDebouncedSave, isConfigLoaded]);

  const setDefaultDownloadPath = (path: string) => {
    _setDownloadPath(path);
  };

  const setCookiesPath = (path: string | null) => {
      _setCookiesPath(path);
      if (path) _setCookiesBrowser(null); 
  };

  const setCookiesBrowser = (browser: string | null) => {
      _setCookiesBrowser(browser);
      if (browser && browser !== 'none') _setCookiesPath(null);
  };

  const setFilenameTemplateBlocks = (blocks: TemplateBlock[]) => {
    _setTemplateBlocks(blocks);
  };

  const setConcurrency = (concurrent: number, total: number) => {
    _setMaxConcurrentDownloads(concurrent);
    _setMaxTotalInstances(total);
  };

  const setLogLevel = (level: string) => {
      _setLogLevel(level);
  };

  const setCheckForUpdates = (enabled: boolean) => {
      _setCheckForUpdates(enabled);
  };

  const updatePreferences = (updates: Partial<PreferenceConfig>) => {
      const newPrefs = { ...preferences, ...updates };
      _setPreferences(newPrefs);
      // Preferences are usually user-interaction driven and less frequent than sliders, can save immediately
      savePreferenceConfig(newPrefs).catch(e => console.error("Failed to save preferences:", e));
  };

  const value = {
    isConfigLoaded,
    isJsRuntimeMissing,
    setIsJsRuntimeMissing,
    isSettingsOpen,
    settingsActiveTab,
    settingsActiveSection,
    openSettings,
    closeSettings,
    setSettingsActiveTab,
    defaultDownloadPath,
    setDefaultDownloadPath,
    filenameTemplateBlocks,
    setFilenameTemplateBlocks,
    getTemplateString,
    cookiesPath,
    setCookiesPath,
    cookiesBrowser,
    setCookiesBrowser,
    maxConcurrentDownloads,
    maxTotalInstances,
    setConcurrency,
    logLevel,
    setLogLevel,
    checkForUpdates,
    setCheckForUpdates,
    isUpdateAvailable,
    latestVersion,
    currentVersion,
    checkAppUpdate,
    preferences,
    updatePreferences
  };

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
};

export const useAppContext = () => {
  const context = React.useContext(AppContext);
  if (context === undefined) {
    throw new Error('useAppContext must be used within an AppProvider');
  }
  return context;
};