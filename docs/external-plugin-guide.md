# External Plugin Development Guide

This guide shows how to create **external plugins** for openNook that are completely independent of the source code.

> [!IMPORTANT]
> External plugins are installed in `~/.opennook/plugins/` and require **no source code changes** to openNook. Users can install them by simply copying to the plugins directory.

## Built-in vs External Plugins

| Type | Location | Requires Source Access | Use Case |
|------|----------|----------------------|----------|
| **Built-in** | `src/components/widgets/` | ✅ Yes | Core widgets (Calendar, Timer) |
| **External** | `~/.opennook/plugins/` | ❌ No | Third-party plugins |

This guide is for **external plugins only**.

---

## Quick Start

### 1. Create Plugin Directory

```bash
mkdir -p ~/.opennook/plugins/my-counter
cd ~/.opennook/plugins/my-counter
```

### 2. Create plugin.json

```json
{
  "id": "my-counter",
  "name": "My Counter",
  "version": "1.0.0",
  "description": "A simple counter widget",
  "author": "Your Name",
  "main": "index.js",
  "category": "utility",
  "minWidth": 200,
  "hasCompactMode": false,
  "permissions": []
}
```

### 3. Create index.js

External plugins must be **pre-bundled**. Here's a minimal example:

```javascript
// index.js - This is your bundled plugin
(function() {
  // The plugin loader provides these globals
  const { registerWidget, WidgetWrapper, icons } = window.__openNookPluginAPI__;
  const { IconBox } = icons;

  // Define your widget component
  function MyCounterWidget() {
    const [count, setCount] = React.useState(0);

    return React.createElement(WidgetWrapper, {
      title: 'My Counter',
      key: 'wrapper'
    }, [
      React.createElement('div', {
        className: 'flex flex-col items-center justify-center gap-4 h-full',
        key: 'content'
      }, [
        React.createElement('h2', {
          className: 'text-5xl font-bold text-white',
          key: 'count'
        }, count),
        React.createElement('button', {
          onClick: () => setCount(c => c + 1),
          className: 'px-6 py-2 bg-blue-500 hover:bg-blue-400 rounded-full text-white transition-colors',
          key: 'button'
        }, 'Increment')
      ])
    ]);
  }

  // Register the widget
  registerWidget({
    id: 'my-counter',
    name: 'My Counter',
    description: 'A simple counter',
    icon: IconBox,
    ExpandedComponent: MyCounterWidget,
    defaultEnabled: false,
    category: 'utility',
    hasCompactMode: false
  });
})();
```

### 4. Test

Restart openNook. Your plugin will be loaded automatically!

---

## Development Workflow

### Option 1: Plain JavaScript (Simple)

Write vanilla JS with React.createElement as shown above. No build step needed.

### Option 2: React + Build Tool (Recommended)

Use a proper development environment:

```bash
# Initialize npm project
npm init -y

# Install dependencies
npm install react react-dom @tabler/icons-react
npm install -D esbuild

# Create tsconfig.json (Important for JSX)
cat > tsconfig.json << 'EOF'
{
  "compilerOptions": {
    "target": "es2020",
    "lib": ["dom", "es2020"],
    "module": "esnext",
    "jsx": "react",
    "jsxFactory": "React.createElement",
    "jsxFragmentFactory": "React.Fragment",
    "strict": true,
    "moduleResolution": "node",
    "types": ["react"]
  },
  "include": ["src/**/*"]
}
EOF

# Create source file
cat > src/index.jsx << 'EOF'
import { useState } from 'react';
import { IconCalculator } from '@tabler/icons-react';

export function MyCounterWidget() {
  const [count, setCount] = useState(0);

  return (
    <div className="flex flex-col items-center gap-4 p-4">
      <h2 className="text-2xl font-bold text-white">{count}</h2>
      <button
        onClick={() => setCount(c => c + 1)}
        className="px-4 py-2 bg-blue-500 rounded-lg text-white"
      >
        Increment
      </button>
    </div>
  );
}

// Register when loaded
if (window.__openNookPluginAPI__) {
  const { registerWidget } = window.__openNookPluginAPI__;
  registerWidget({
    id: 'my-counter',
    name: 'My Counter',
    description: 'A simple counter',
    icon: IconCalculator,
    ExpandedComponent: MyCounterWidget,
    defaultEnabled: false,
    category: 'utility',
    hasCompactMode: false
  });
}
EOF

# Build script
cat > build.js << 'EOF'
require('esbuild').build({
  entryPoints: ['src/index.jsx'],
  bundle: true,
  outfile: 'index.js',
  format: 'iife',
  globalName: '__plugin__',
  external: ['react', 'react-dom'],
  jsxFactory: 'React.createElement',
  jsxFragment: 'React.Fragment',
}).catch(() => process.exit(1));
EOF

# Build
node build.js
```

---

## Plugin API

### Registration

```javascript
registerWidget({
  id: string,              // Unique ID
  name: string,            // Display name
  description: string,     // Description
  icon: ComponentType,     // Icon from @tabler/icons-react
  ExpandedComponent: ComponentType,
  CompactComponent?: ComponentType,
  defaultEnabled: boolean,
  category: 'productivity' | 'media' | 'utility',
  minWidth?: number,
  hasCompactMode: boolean,
  compactPriority?: number
});
```

### Available Globals

External plugins have access to a comprehensive API via `window.__openNookPluginAPI__`:

```javascript
window.__openNookPluginAPI__ = {
  // Core
  registerWidget,    // Register function
  React,            // React library

  // UI Components
  WidgetWrapper,    // Standard widget container
  WidgetAddDialog,  // Dialog for adding items
  Button,           // Shadcn button component
  Popover,          // Popover component
  PopoverTrigger,   // Popover trigger
  PopoverContent,   // Popover content
  PopoverAnchor,    // Popover anchor

  // Hooks
  useNotification,  // Show notifications (hydration, screenBreak)

  // Icons - 25+ commonly used icons from @tabler/icons-react
  icons: {
    IconBox, IconPlus, IconTrash, IconRefresh, IconClock,
    IconCalendar, IconPlayerPlay, IconPlayerPause, IconPlayerStop,
    IconCheck, IconX, IconSettings, IconBell, IconStar,
    IconHeart, IconHome, IconUser, IconSearch, IconMenu,
    IconChevronDown, IconChevronUp, IconChevronLeft, IconChevronRight,
    IconLoader, IconAlertCircle, IconInfoCircle, IconCircleCheck
  },

  // Backend API
  invoke,           // Call Tauri backend commands
};
```

### Styling

Use Tailwind CSS utility classes. Common patterns:

```javascript
// Card-style container
className="flex flex-col gap-4 p-4 bg-white/10 rounded-lg"

// Button
className="px-4 py-2 bg-blue-500 hover:bg-blue-400 rounded-lg text-white"

// Text
className="text-white text-lg font-semibold"
```

---

## Example: Weather Widget

```javascript
(function() {
  const { registerWidget, WidgetWrapper, React, icons } = window.__openNookPluginAPI__;
  const { IconCloud } = icons;
  const { useState, useEffect } = React;

  function WeatherWidget() {
    const [weather, setWeather] = useState(null);

    useEffect(() => {
      // Fetch weather data
      fetch('https://api.weather.gov/...')
        .then(r => r.json())
        .then(setWeather);
    }, []);

    return React.createElement(WidgetWrapper, {
      title: 'Weather'
    }, [
      React.createElement('div', {
        className: 'flex flex-col items-center justify-center h-full gap-2',
        key: 'content'
      }, [
        weather ? React.createElement('p', {
          className: 'text-3xl text-white font-bold',
          key: 'temp'
        }, `${weather.temperature}°F`) : React.createElement('p', {
          className: 'text-white/60',
          key: 'loading'
        }, 'Loading...')
      ])
    ]);
  }

  registerWidget({
    id: 'weather',
    name: 'Weather',
    description: 'Show current weather',
    icon: IconCloud,
    ExpandedComponent: WeatherWidget,
    defaultEnabled: false,
    category: 'utility',
    hasCompactMode: false
  });
})();
```

---

## Publishing

1. Bundle your plugin (if using build tools)
2. Create a GitHub repo with:
   - `plugin.json`
   - `index.js` (bundled)
   - `README.md`
3. Users install by cloning to `~/.opennook/plugins/`

---

## Limitations

- Frontend-only (no Rust/backend access)
- Must use pre-bundled JavaScript
- React, @tabler/icons-react provided globally

For full backend integration, plugins must be built-in widgets.
