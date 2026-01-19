# Repository Metadata

This document contains the recommended description and topics for the openNook GitHub repository.

## Repository Description

```
An open-source dynamic island client for desktop. Built with Tauri, React, and TypeScript, bringing the utility and aesthetic of the dynamic island to macOS and Windows.
```

## Repository Topics

The following topics should be added to improve discoverability:

- `tauri`
- `react`
- `desktop-app`
- `typescript`
- `dynamic-island`
- `macos`
- `windows`
- `cross-platform`
- `plugin-system`
- `rust`
- `motion`

## How to Update

Repository description and topics can be updated through:

1. **GitHub Web Interface:**
   - Go to the repository settings
   - Update the "Description" field at the top
   - Add topics in the "Topics" section

2. **GitHub CLI:**
   ```bash
   # Update description
   gh repo edit prodBirdy/openNook --description "An open-source dynamic island client for desktop. Built with Tauri, React, and TypeScript, bringing the utility and aesthetic of the dynamic island to macOS and Windows."
   
   # Add topics
   gh repo edit prodBirdy/openNook --add-topic tauri,react,desktop-app,typescript,dynamic-island,macos,windows,cross-platform,plugin-system,rust,motion
   ```

3. **GitHub API:**
   ```bash
   # Update description and topics
   curl -X PATCH \
     -H "Accept: application/vnd.github+json" \
     -H "Authorization: Bearer YOUR_TOKEN" \
     https://api.github.com/repos/prodBirdy/openNook \
     -d '{"description":"An open-source dynamic island client for desktop. Built with Tauri, React, and TypeScript, bringing the utility and aesthetic of the dynamic island to macOS and Windows.","topics":["tauri","react","desktop-app","typescript","dynamic-island","macos","windows","cross-platform","plugin-system","rust","motion"]}'
   ```
