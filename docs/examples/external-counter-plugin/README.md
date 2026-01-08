# External Counter Plugin

Example external plugin for openNook using **TSX** and the openNook UI components.

## Installation

1. Copy this entire folder to `~/.opennook/plugins/`:
   ```bash
   mkdir -p ~/.opennook/plugins
   cp -r external-counter-plugin ~/.opennook/plugins/
   ```

2. Restart openNook (or use Plugin Store to install)

3. Enable "External Counter" in Settings

## Development

### Prerequisites
- Node.js 18+
- npm or bun

### Setup
```bash
npm install
```

### Build
```bash
npm run build
```

This compiles `src/index.tsx` → `index.js`

### Workflow
1. Edit `src/index.tsx`
2. Run `npm run build`
3. Restart openNook (or delete + reinstall plugin)

## Files

```
external-counter-plugin/
├── plugin.json       # Plugin manifest
├── package.json      # npm config with build script
├── src/
│   └── index.tsx     # TSX source (edit this!)
├── index.js          # Built bundle (auto-generated)
└── README.md         # This file
```

## Available API

External plugins have access to:

```typescript
const {
    registerWidget,      // Register your widget
    React,              // React library
    WidgetWrapper,      // Standard widget container
    WidgetAddDialog,    // Form dialog component
    IconBox,            // Default icon
} = window.__openNookPluginAPI__;
```

## Using WidgetWrapper

The `WidgetWrapper` component provides consistent styling:

```tsx
function MyWidget() {
    return (
        <WidgetWrapper title="My Widget" icon={MyIcon}>
            {/* Your content here */}
        </WidgetWrapper>
    );
}
```