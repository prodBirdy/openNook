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

*   **Cross-Platform Support**: While currently focused on macOS, we plan to leverage Tauri's capabilities to bring openNook to Windows and Linux users in the future.
*   **Plugin Ecosystem**: We are planning a Plugin API that will empower users and developers to extend the app. You will be able to create:
    *   **Custom Widgets**: Add new functionality tailored to your needs.
    *   **New Tabs**: extend the interface with new pages.
    *   **Custom Interfaces**: Redesign or repurpose the island for different workflows.

## Getting Started

To run this project locally:

1.  Make sure you have prerequisites for [Tauri](https://tauri.app/v1/guides/getting-started/prerequisites) installed.
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

## Contributing

This project is open source and we welcome contributions! Whether it's fixing bugs, improving the UI, or suggesting new features for the upcoming Plugin API, your help is appreciated.
