declare module 'fast-speedtest-api' {
    export default class FastSpeedtest {
        constructor(options: {
            token?: string;
            verbose?: boolean;
            timeout?: number;
            https?: boolean;
            urlCount?: number;
            bufferSize?: number;
            unit?: string;
        });

        static readonly UNITS: {
            Bps: string;
            KBps: string;
            MBps: string;
            GBps: string;
            Kbps: string;
            Mbps: string;
            Gbps: string;
        };

        getSpeed(): Promise<number>;
    }
}
