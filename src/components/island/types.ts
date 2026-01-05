export interface NowPlayingData {
    title: string | null;
    artist: string | null;
    album: string | null;
    artwork_base64: string | null;
    duration: number | null;
    elapsed_time: number | null;
    is_playing: boolean;
    audio_levels: number[] | null;
    app_name: string | null;
}
