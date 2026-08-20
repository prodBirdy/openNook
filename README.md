**Windows:** [Download and run](https://github.com/prodBirdy/openNook/releases/download/0.0.2a/openNook_0.1.0_x64-setup.exe)

# <img width="32" height="32" alt="Subject" src="https://github.com/user-attachments/assets/5f5b3858-2055-4b66-b3ca-f9206eb19247" /> openNook 

openNook is an open-source dynamic island client inspired by [notchNook](https://lo.cafe/notchnook). It brings the utility and aesthetic of the dynamic island to your desktop, currently built with performance and design in mind.


https://github.com/user-attachments/assets/53ddaf37-2576-4b62-b367-5ca2a96e9cb2


## About The Project

openNook aims to provide a seamless and interactive "island" experience on your screen. It serves as a hub for media controls, widgets (like Calendar and Reminders), shortcuts, and file management, all accessible from a sleek, expanding pill at the top of your display.

## Built With

*   **[Tauri](https://tauri.app/)**: Providing a lightweight, secure, and performant backend using Rust.
*   **[React](https://react.dev/)**: Powering the frontend user interface.
*   **[Motion](https://motion.dev/)**: Enabling fluid, high-quality animations and interactions.

## Roadmap & Plans

We are just getting started. The goal is to evolve openNook into a highly extensible platform:

*   **Cross-Platform Support**: openNook currently supports macOS and Windows. Validated on Windows 11.
*   **Plugin Ecosystem**: The core Plugin API is implemented, empowering users and developers to extend the app. You can currently create:
    *   **Custom Widgets**: Add new functionality tailored to your needs.
    *   **New Tabs**: extend the interface with new pages.
    *   **Custom Interfaces**: Redesign or repurpose the island for different workflows.


## Plugin System

openNook features a plugin system that allows you to load external functionality.

*   **Installation**: Plugins can be installed from a local folder or a Git repository.
*   **Location**: Plugins are stored in `~/.opennook/plugins`.
*   **Development**: Developers can create plugins as separate JavaScript/TypeScript bundles.

## Getting Started

To run this project locally:

> [!NOTE]
> Windows users, please check [WINDOWS_BUILD.md](WINDOWS_BUILD.md) for specific instructions.

1.  Make sure you have prerequisites for [Tauri](https://tauri.app/start/prerequisites/) installed.
2.  Install dependencies:
    ```bash
    npm install
    # or
    bun i
    ```
3.  Run the development server:
    ```bash
    npm run tauri dev
    # or
    bun run tauri dev
    ```

## Releases

Releases are automatically built and published via GitHub Actions when code is pushed to the `main` branch or when a version tag is created. The CI/CD workflow builds cross-platform binaries for:

- macOS (Apple Silicon and Intel)
- Windows
- Linux (Ubuntu)

Draft releases are created automatically with all platform binaries attached. You can find releases on the [releases page](https://github.com/prodBirdy/openNook/releases).

## Contributing

This project is open source and we welcome contributions! Whether it's fixing bugs, improving the UI, or suggesting new features for the upcoming Plugin API, your help is appreciated.
