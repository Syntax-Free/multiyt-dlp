import { logFrontendMessage } from "@/api/invoke";

export enum LogLevel {
    Info = "Info",
    Warn = "Warn",
    Error = "Error",
    Debug = "Debug",
}

/**
 * Frontend Logger Wrapper
 * Sends logs to the Rust backend to be included in latest.log
 */
export const logger = {
    info: (msg: string, context?: string) => {
        // Log to browser console for dev
        console.info(`[${context || 'Frontend'}] ${msg}`);
        // Send to backend
        logFrontendMessage(LogLevel.Info, msg, context).catch(e => console.error("Failed to log to backend", e));
    },

    warn: (msg: string, context?: string) => {
        console.warn(`[${context || 'Frontend'}] ${msg}`);
        logFrontendMessage(LogLevel.Warn, msg, context).catch(e => console.error("Failed to log to backend", e));
    },

    error: (msg: string, context?: string) => {
        console.error(`[${context || 'Frontend'}] ${msg}`);
        logFrontendMessage(LogLevel.Error, msg, context).catch(e => console.error("Failed to log to backend", e));
    },

    debug: (msg: string, context?: string) => {
        console.debug(`[${context || 'Frontend'}] ${msg}`);
        logFrontendMessage(LogLevel.Debug, msg, context).catch(e => console.error("Failed to log to backend", e));
    }
};