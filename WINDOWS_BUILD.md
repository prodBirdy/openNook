# Windows Build Guide

This document provides instructions for building and running openNook on Windows.

## Prerequisites

1. **Rust and Cargo**: Install from [rustup.rs](https://rustup.rs/)
2. **Node.js**: Version 18+ (or use Bun as shown in the main README)
3. **Windows SDK**: Required for Windows API development
   - Install via Visual Studio Build Tools or Visual Studio
   - Ensure "Desktop development with C++" workload is installed
4. **WebView2**: Usually pre-installed on Windows 11, download runtime if needed

## Building

1. Install dependencies:
   ```bash
   npm install
   # or
   bun install
   ```

2. Build and run in development mode:
   ```bash
   npm run tauri dev
   # or
   bun run tauri dev
   ```

3. Build for production:
   ```bash
   npm run tauri build
   # or
   bun run tauri build
   ```

## Windows-Specific Features

### Window Positioning
- The application window is positioned at the top-center of the screen
- Window is frameless and always-on-top
- Click-through support when mouse is not hovering over UI elements

### Media Controls
- Integrates with Windows GlobalSystemMediaTransportControls API
- Works with any media player that supports Windows media controls (Spotify, browser media, etc.)
- Supports play/pause, next track, previous track

### File Operations
- **Open File**: Uses `cmd /C start` to open files with default application
- **Reveal in Explorer**: Uses `explorer /select` to show file in Windows Explorer

## Platform Differences from macOS

### Not Implemented on Windows
- **Native Calendar Integration**: Windows doesn't have a direct equivalent to macOS EventKit
  - Calendar functions return empty arrays
  - Can be extended using Microsoft Graph API in the future
  
- **Haptic Feedback**: Windows doesn't have system-wide haptic feedback API
  - Function returns success but performs no action
  - Could be extended with device-specific haptic libraries

- **Notch Detection**: Windows PCs don't have notches
  - Window positioning uses screen dimensions instead
  - Can be configured via settings for optimal placement

### Fully Implemented on Windows
- ✅ Window management and positioning
- ✅ Mouse hover detection and click-through
- ✅ Media playback controls
- ✅ Audio visualization (simulated)
- ✅ File operations (open, reveal)
- ✅ Settings persistence
- ✅ Notes and widgets
- ✅ Plugin system
- ✅ Database storage

## Troubleshooting

### Build Errors

**Error: "cannot find -lwindows"**
- Ensure Windows SDK is installed
- Restart terminal after SDK installation

**Error: "failed to run custom build command"**
- Clear cargo cache: `cargo clean`
- Rebuild: `cargo build`

**WebView2 Runtime Missing**
- Download from [Microsoft](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)

### Runtime Issues

**Window not appearing**
- Check if window is behind other windows (should be always-on-top)
- Try adjusting window position settings

**Media controls not working**
- Ensure media is playing in a compatible app (Spotify, browser, etc.)
- Check Windows privacy settings for media control access

## Development Notes

### Code Structure
- Platform-specific code uses `#[cfg(target_os = "windows")]` guards
- Windows implementations parallel macOS implementations where possible
- Error handling uses `Result<T, String>` for consistent error reporting

### Testing
- Test window positioning on different screen resolutions
- Verify media controls with multiple media sources
- Test file operations with various file types

## Future Enhancements

Potential areas for Windows-specific improvements:
- [ ] Native calendar integration via Microsoft Graph API
- [ ] Windows notification integration
- [ ] Taskbar thumbnail customization
- [ ] Windows Hello integration for settings
- [ ] Better multi-monitor support
- [ ] Windows accent color integration (currently uses default)
