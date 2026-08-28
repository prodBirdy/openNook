
// Create a simple pleasant notification sound using Web Audio API
// This avoids needing an external mp3 file
export function playNotificationSound() {
    try {
        const AudioContext = window.AudioContext || (window as any).webkitAudioContext;
        if (!AudioContext) return;

        const ctx = new AudioContext();

        // Create a simple "Ding" like a bell or elevator chime
        // Sine wave is smooth
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();

        osc.connect(gain);
        gain.connect(ctx.destination);

        osc.type = 'sine';

        // nice harmonic E5 -> C6?? Or just a single nice distinct tone.
        // Let's do a fast arpeggio or a single "Ding"
        // 880Hz (A5) fading out looks nice.

        const now = ctx.currentTime;

        osc.frequency.setValueAtTime(523.25, now); // C5
        osc.frequency.exponentialRampToValueAtTime(1046.5, now + 0.1); // Jump to C6 quickly for a "ping" attack

        // Envelope
        gain.gain.setValueAtTime(0, now);
        gain.gain.linearRampToValueAtTime(0.3, now + 0.05); // Attack
        gain.gain.exponentialRampToValueAtTime(0.001, now + 1.5); // Decay

        osc.start(now);
        osc.stop(now + 1.5);

        // Add a second harmonic for richness
        const osc2 = ctx.createOscillator();
        const gain2 = ctx.createGain();
        osc2.connect(gain2);
        gain2.connect(ctx.destination);
        osc2.type = 'sine';
        osc2.frequency.setValueAtTime(523.25 * 2, now); // C6 naturally

        gain2.gain.setValueAtTime(0, now);
        gain2.gain.linearRampToValueAtTime(0.1, now + 0.05);
        gain2.gain.exponentialRampToValueAtTime(0.001, now + 1.2);

        osc2.start(now);
        osc2.stop(now + 1.2);

    } catch (e) {
        console.error('Failed to play sound', e);
    }
}
