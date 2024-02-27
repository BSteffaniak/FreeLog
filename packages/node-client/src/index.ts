export type LogComponentPrimitive =
    | string
    | number
    | boolean
    | undefined
    | null;
export type LogComponent = LogComponentPrimitive | LogComponentPrimitive[];
export type LogLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';

type PartialBy<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;

export type Config = {
    logWriterApiUrl: string;
    logLevel: LogLevel;
    shimConsole: boolean;
    autoFlush: boolean;
    autoFlushOnClose: boolean;
};

type LogEntry = { level: LogLevel; values: LogComponent[]; ts: number };

const logBuffer: LogEntry[] = [];
let flushTimeout: ReturnType<typeof setTimeout> | undefined;
let bufferSize = 0;
let initialized = false;
const defaultConfig: Config = {
    logWriterApiUrl: '',
    logLevel: 'ERROR',
    shimConsole: false,
    autoFlush: true,
    autoFlushOnClose: true,
};
const config: Config = defaultConfig;

const levels: Record<LogLevel, number> = {
    TRACE: 0,
    DEBUG: 1,
    INFO: 2,
    WARN: 3,
    ERROR: 4,
} as const;

const oldConsole = {
    trace: console.trace, // eslint-disable-line no-console
    debug: console.debug, // eslint-disable-line no-console
    log: console.log, // eslint-disable-line no-console
    warn: console.warn, // eslint-disable-line no-console
    error: console.error, // eslint-disable-line no-console
};

const defaultLogger = {
    TRACE: oldConsole.debug,
    DEBUG: oldConsole.debug,
    INFO: oldConsole.log,
    WARN: oldConsole.warn,
    ERROR: oldConsole.error,
};

function circularStringify(obj: object): string {
    const getCircularReplacer = () => {
        const seen = new WeakSet();
        return (_key: string, value: unknown) => {
            if (typeof value === 'object' && value !== null) {
                if (seen.has(value)) {
                    return '[[circular]]';
                }
                seen.add(value);
            }
            return value;
        };
    };

    return JSON.stringify(obj, getCircularReplacer());
}

export function objToStr(obj: unknown): string {
    if (typeof obj === 'string') {
        return obj;
    }
    if (typeof obj === 'undefined') {
        return 'undefined';
    }
    if (obj === null) {
        return 'null';
    }
    if (obj instanceof Error) {
        if (obj.stack) {
            return obj.stack;
        } else {
            return `${obj}`;
        }
    }
    if (typeof obj === 'object') {
        return circularStringify(obj);
    }

    return obj.toString();
}

export function init(
    opts: PartialBy<
        Config,
        'logLevel' | 'shimConsole' | 'autoFlush' | 'autoFlushOnClose'
    >,
) {
    initialized = true;

    const options = structuredClone(defaultConfig);
    Object.assign(options, opts);
    Object.assign(config, options);

    if (config.shimConsole) {
        shimConsole();
    }
    if (!config.autoFlushOnClose) {
        autoFlushOnClose();
    }
}

function calculateLogComponentSize(value: LogComponent): number {
    if (value === null) return 4;
    if (value === undefined) return 9;
    if (value === true) return 4;
    if (value === false) return 5;

    return value.toString().length;
}

function calculateLogComponentsSize(values: LogComponent[]): number {
    let sum = 0;

    values
        .map((component) => calculateLogComponentSize(component))
        .forEach((size) => {
            sum += size;
        });

    return sum;
}

function flushAfterDelay(delay: number = 1000) {
    if (flushTimeout) {
        clearTimeout(flushTimeout);
    }

    flushTimeout = setTimeout(flush, delay);
}

export async function flush() {
    if (!initialized) throw new Error(`Logger not initialized`);

    if (flushTimeout) {
        clearTimeout(flushTimeout);
    }

    flushTimeout = undefined;

    if (bufferSize > 0) {
        const body = JSON.stringify(logBuffer);
        logBuffer.length = 0;
        bufferSize = 0;

        await fetch(`${config.logWriterApiUrl}/logs`, {
            method: 'POST',
            body,
            headers: { 'content-type': 'application/json' },
        });
    }
}

function writeLog(level: LogLevel, values: LogComponent[]) {
    if (!initialized) throw new Error(`Logger not initialized`);
    if (levels[level] < levels[config.logLevel]) return;

    logBuffer.push({ level, values, ts: Date.now() });
    bufferSize += calculateLogComponentsSize(values);

    if (bufferSize > 10240) {
        flush();
    } else {
        flushAfterDelay();
    }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function toLogComponent(value: any): LogComponent {
    if (value === null) return value;

    const type = typeof value;

    switch (type) {
        case 'string':
        case 'number':
        case 'undefined':
            return value as LogComponent;
        default:
            return objToStr(value);
    }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function log(this: void, level: LogLevel, ...args: any[]) {
    defaultLogger[level]?.apply(this, [`[${level.toUpperCase()}]`, ...args]);
    const components = args.map(toLogComponent);
    logComponents(level, components);
}

export function logComponents(level: LogLevel, components: LogComponent[]) {
    writeLog(level, components);
}

function shimConsole() {
    // eslint-disable-next-line no-console, @typescript-eslint/no-explicit-any
    console.trace = function (...args: any[]) {
        log('TRACE', ...args);
    };

    // eslint-disable-next-line no-console, @typescript-eslint/no-explicit-any
    console.debug = function (...args: any[]) {
        log('DEBUG', ...args);
    };

    // eslint-disable-next-line no-console, @typescript-eslint/no-explicit-any
    console.log = function (...args: any[]) {
        log('INFO', ...args);
    };

    // eslint-disable-next-line no-console, @typescript-eslint/no-explicit-any
    console.warn = function (...args: any[]) {
        log('WARN', ...args);
    };

    // eslint-disable-next-line no-console, @typescript-eslint/no-explicit-any
    console.error = function (...args: any[]) {
        log('ERROR', ...args);
    };
}

function onBeforeUnload() {
    flush();
}

function autoFlushOnClose() {
    // @ts-ignore
    if (typeof window !== 'undefined') {
        // @ts-ignore
        window.addEventListener('beforeunload', onBeforeUnload);

        // @ts-ignore
        const { meta } = window.import;

        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        if ('hot' in meta) {
            // @ts-ignore
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            (meta as any).hot?.on('vite:beforeUpdate', onBeforeUnload);
        }
    }
}
