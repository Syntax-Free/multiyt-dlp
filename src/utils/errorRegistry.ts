import { ErrorPattern } from "@/types";

export const ERROR_PATTERNS: ErrorPattern[] = [
    // --- Authentication & Bot Detection ---
    {
        id: "bot_detection",
        pattern: /(Sign in to confirm|confirm you'?re not a bot|returned non-zero exit status 1)/i,
        title: "Bot Detection Triggered",
        description: "YouTube requires a browser sign-in to verify you are not a bot.",
        actionLabel: "Configure Cookies",
        actionType: "OPEN_SETTINGS",
        actionTarget: "ytdlp:section-cookies"
    },
    {
        id: "http_forbidden",
        pattern: /(HTTP Error 403|Forbidden|Private video)/i,
        title: "Access Denied",
        description: "The video is private, age-restricted, or blocked. Try importing cookies.",
        actionLabel: "Configure Cookies",
        actionType: "OPEN_SETTINGS",
        actionTarget: "ytdlp:section-cookies"
    },
    {
        id: "rate_limit",
        pattern: /HTTP Error 429/i,
        title: "Rate Limit Exceeded",
        description: "Too many requests. Please wait or reduce concurrency.",
        actionLabel: "Adjust Queue Settings",
        actionType: "OPEN_SETTINGS",
        actionTarget: "general:section-queue"
    },
    {
        id: "login_required",
        pattern: /(Sign in required|This video is only available to members)/i,
        title: "Login Required",
        description: "This video requires a premium account or membership.",
        actionLabel: "Configure Cookies",
        actionType: "OPEN_SETTINGS",
        actionTarget: "ytdlp:section-cookies"
    },
    {
        id: "membership_required",
        pattern: /Join this channel/i,
        title: "Membership Required",
        description: "This is a members-only video.",
        actionLabel: "Configure Cookies",
        actionType: "OPEN_SETTINGS",
        actionTarget: "ytdlp:section-cookies"
    },
    {
        id: "age_restricted",
        pattern: /This video is age-restricted/i,
        title: "Age Restricted",
        description: "Sign-in required to verify age.",
        actionLabel: "Configure Cookies",
        actionType: "OPEN_SETTINGS",
        actionTarget: "ytdlp:section-cookies"
    },
    
    // --- System & Network ---
    {
        id: "missing_ffmpeg",
        pattern: /(ffprobe|ffmpeg) not found/i,
        title: "Missing FFmpeg",
        description: "FFmpeg is required for high-quality video merging.",
        actionLabel: "Check Dependencies",
        actionType: "OPEN_SETTINGS",
        actionTarget: "about:section-deps"
    },
    {
        id: "network_error",
        pattern: /(Network problem|Connection reset|timed out|EOF occurred in violation of protocol)/i,
        title: "Network Error",
        description: "Connection was interrupted. Please check your internet.",
    },
    {
        id: "write_error",
        pattern: /(Permission denied|No space left on device|read-only file system)/i,
        title: "Storage Error",
        description: "Cannot write to the download folder. Check permissions or disk space.",
        actionLabel: "Change Folder",
        actionType: "OPEN_SETTINGS",
        actionTarget: "ytdlp:section-formatting"
    },

    // --- Content & Extraction ---
    {
        id: "content_unavailable",
        pattern: /(Video unavailable|This video has been removed|Fragment \d+ not received)/i,
        title: "Content Unavailable",
        description: "The video might be deleted or the stream is corrupt.",
    },
    {
        id: "extractor_error",
        pattern: /(unable to extract|Unsupported URL)/i,
        title: "Extraction Failed",
        description: "The URL format is not supported or the site layout changed.",
        actionLabel: "Check for Updates",
        actionType: "OPEN_SETTINGS",
        actionTarget: "about:section-deps"
    }
];

export interface ParsedError {
    title: string;
    description: string;
    actionLabel?: string;
    actionType?: string;
    actionTarget?: string;
    rawMatches: boolean;
}

export function parseError(stderr: string = "", errorMsg: string = ""): ParsedError {
    // Combine logs to search against
    const combined = (errorMsg + "\n" + stderr).trim();
    
    // 1. Try to match specific "Smart" patterns
    for (const entry of ERROR_PATTERNS) {
        if (entry.pattern.test(combined)) {
            return {
                title: entry.title,
                description: entry.description,
                actionLabel: entry.actionLabel,
                actionType: entry.actionType,
                actionTarget: entry.actionTarget,
                rawMatches: true
            };
        }
    }

    // 2. Fallback Logic:
    // If no regex matched, we MUST show the actual stderr because "Validation Failed" implies nothing to the user.
    let description = errorMsg;
    let title = "Download Failed";

    if (stderr && stderr.trim().length > 0) {
        // Clean up stderr to remove technical noise (like python tracebacks) if possible
        const lines = stderr.split('\n');
        // Filter out python internals to get the "meat" of the error
        const relevantLines = lines.filter(l => 
            !l.includes('Traceback (most recent call last)') && 
            !l.trim().startsWith('File "') &&
            !l.includes('yt_dlp.utils.DownloadError')
        );
        
        const cleanStderr = relevantLines.join(' ').trim();
        
        if (cleanStderr.length > 0) {
            // Use the raw error output, truncated
            description = cleanStderr.length > 180 ? cleanStderr.substring(0, 177) + "..." : cleanStderr;
        }
    }

    return {
        title,
        description: description || "An unknown error occurred. Check logs.",
        rawMatches: false
    };
}

/**
 * Unwraps Tauri/Rust serialized error objects into usable strings.
 * Handles inputs like: {"ProcessFailed":{"exit_code":1,"stderr":"..."}}
 */
export function extractErrorDetails(err: any): { message: string, stderr?: string } {
    let errorObj = err;

    // 1. Try parsing if it's a JSON string
    if (typeof err === 'string') {
        try {
            if (err.trim().startsWith('{')) {
                errorObj = JSON.parse(err);
            } else {
                 return { message: err };
            }
        } catch {
            return { message: err };
        }
    }

    // 2. Handle known Rust Enum Variants
    if (errorObj && typeof errorObj === 'object') {
        if ('ProcessFailed' in errorObj) {
            const { stderr } = errorObj.ProcessFailed;
            // The stderr usually contains the "Sign in to confirm..." text.
            // We pass "Validation Failed" as the short title, but stderr carries the weight.
            return { message: "Validation Failed", stderr };
        }
        if ('IoError' in errorObj) {
            return { message: errorObj.IoError };
        }
        if ('ValidationFailed' in errorObj) {
            return { message: errorObj.ValidationFailed };
        }
        
        // Fallback for generic object
        return { message: JSON.stringify(errorObj) };
    }

    return { message: String(err) };
}