
import { invoke } from '@tauri-apps/api/core';

export interface DatabaseService {
    execute(sql: string, args?: any[]): Promise<number>;
    select<T = any>(sql: string, args?: any[]): Promise<T[]>;
    initTable(tableName: string, schema: string): Promise<void>;
    getSetting<T>(key: string): Promise<T | null>;
    saveSetting(key: string, value: any): Promise<void>;
}

class TauriDatabaseService implements DatabaseService {
    async execute(sql: string, args: any[] = []): Promise<number> {
        return await invoke('db_execute', { sql, args });
    }

    async select<T = any>(sql: string, args: any[] = []): Promise<T[]> {
        return await invoke('db_select', { sql, args });
    }

    async initTable(tableName: string, schema: string): Promise<void> {
        // Basic protection against simple injection in table name, though user provided
        const safeTableName = tableName.replace(/[^a-zA-Z0-9_]/g, '');
        const createSql = `CREATE TABLE IF NOT EXISTS ${safeTableName} (${schema})`;
        await this.execute(createSql);
    }

    async getSetting<T>(key: string): Promise<T | null> {
        const result = await this.select<{ value: string }>(
            "SELECT value FROM settings WHERE key = $1",
            [key]
        );

        if (result.length === 0) return null;

        try {
            return JSON.parse(result[0].value) as T;
        } catch (e) {
            console.error(`Failed to parse setting for key ${key}:`, e);
            return null;
        }
    }

    async saveSetting(key: string, value: any): Promise<void> {
        const jsonValue = JSON.stringify(value);
        await this.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ($1, $2)",
            [key, jsonValue]
        );
    }
}

export const dbService = new TauriDatabaseService();
