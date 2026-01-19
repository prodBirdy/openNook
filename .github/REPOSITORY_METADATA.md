# Repository Metadata Configuration

> **Action Required**: This document contains the metadata that needs to be applied to the GitHub repository. Please follow the instructions below to update the repository description and topics.

## Current Status

- **Description**: ❌ Not set (currently `null`)
- **Topics**: ❌ Not set

## Required Repository Description

```
An open-source dynamic island client for desktop. Built with Tauri, React, and TypeScript, bringing the utility and aesthetic of the dynamic island to macOS and Windows.
```

## Required Repository Topics

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

## How to Apply These Changes

### Option 1: Using the Provided Script (Recommended)

Run the automated script:
```bash
.github/apply-metadata.sh
```

This script will update both the description and topics automatically using GitHub CLI.

### Option 2: Manual Methods

Repository description and topics can also be updated manually through:

1. **GitHub Web Interface (No CLI required):**
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
