# macOS Accessibility Tools Guide

**Last updated**: January 2025  
**Source of truth**: `crates/g3-computer-control/src/macax/`

## Purpose

G3 includes tools for controlling macOS applications via the Accessibility API. This enables automation of native macOS apps, including those you're building with G3.

## Overview

The macOS Accessibility API provides programmatic access to UI elements in any application. G3 exposes this through the `macax_*` tools, allowing you to:

- List and activate applications
- Inspect UI element hierarchies
- Find elements by role, title, or identifier
- Click buttons and interact with controls
- Read and set values in text fields
- Simulate keyboard input

## Setup

### 1. Enable in Configuration

```toml
# ~/.config/g3/config.toml
[macax]
enabled = true
```

Or use the CLI flag:

```bash
g3 --macax
```

### 2. Grant Accessibility Permissions

1. Open **System Preferences** → **Security & Privacy** → **Privacy**
2. Select **Accessibility** in the left sidebar
3. Click the lock icon and authenticate
4. Add your terminal application (Terminal, iTerm2, etc.)
5. Restart your terminal

**Note**: If using VS Code's integrated terminal, add VS Code to the list.

### 3. Verify Setup

```json
{"tool": "macax_list_apps", "args": {}}
```

This should return a list of running applications.

## Available Tools

### macax_list_apps

List all running applications.

**Parameters**: None

**Example**:
```json
{"tool": "macax_list_apps", "args": {}}
```

**Returns**:
```
Running Applications:
- Safari (com.apple.Safari)
- Finder (com.apple.finder)
- Terminal (com.apple.Terminal)
- MyApp (com.example.myapp)
```

---

### macax_get_frontmost_app

Get the currently active (frontmost) application.

**Parameters**: None

**Example**:
```json
{"tool": "macax_get_frontmost_app", "args": {}}
```

**Returns**:
```
Frontmost Application: Safari (com.apple.Safari)
```

---

### macax_activate_app

Bring an application to the front.

**Parameters**:
- `app_name` (string, required): Application name

**Example**:
```json
{"tool": "macax_activate_app", "args": {"app_name": "Safari"}}
```

---

### macax_get_ui_tree

Get the UI element hierarchy of an application.

**Parameters**:
- `app_name` (string, required): Application name
- `max_depth` (integer, optional): Maximum tree depth (default: 5)

**Example**:
```json
{"tool": "macax_get_ui_tree", "args": {"app_name": "Calculator", "max_depth": 3}}
```

**Returns**:
```
UI Tree for Calculator:
└── AXApplication "Calculator"
    └── AXWindow "Calculator"
        ├── AXGroup
        │   ├── AXButton "1" [id: digit_1]
        │   ├── AXButton "2" [id: digit_2]
        │   ├── AXButton "+" [id: add]
        │   └── AXButton "=" [id: equals]
        └── AXStaticText "0" [id: display]
```

**Notes**:
- Use lower `max_depth` for complex apps to avoid overwhelming output
- Elements show role, title, and accessibility identifier (if set)

---

### macax_find_elements

Find UI elements matching criteria.

**Parameters**:
- `app_name` (string, required): Application name
- `role` (string, optional): Element role (e.g., "button", "textField")
- `title` (string, optional): Element title/label
- `identifier` (string, optional): Accessibility identifier

**Example**:
```json
{"tool": "macax_find_elements", "args": {
  "app_name": "Safari",
  "role": "button"
}}
```

**Returns**:
```
Found 5 elements:
1. AXButton "Back" [id: BackButton]
2. AXButton "Forward" [id: ForwardButton]
3. AXButton "Reload" [id: ReloadButton]
4. AXButton "Share" [id: ShareButton]
5. AXButton "New Tab" [id: NewTabButton]
```

---

### macax_click

Click a UI element.

**Parameters**:
- `app_name` (string, required): Application name
- `identifier` (string, optional): Accessibility identifier
- `title` (string, optional): Element title
- `role` (string, optional): Element role

At least one of `identifier`, `title`, or `role` must be provided.

**Examples**:

```json
// Click by identifier (most reliable)
{"tool": "macax_click", "args": {
  "app_name": "Calculator",
  "identifier": "digit_5"
}}

// Click by title
{"tool": "macax_click", "args": {
  "app_name": "Calculator",
  "title": "5"
}}

// Click by role and title
{"tool": "macax_click", "args": {
  "app_name": "Safari",
  "role": "button",
  "title": "Reload"
}}
```

---

### macax_set_value

Set the value of a UI element (text fields, sliders, etc.).

**Parameters**:
- `app_name` (string, required): Application name
- `identifier` (string, optional): Accessibility identifier
- `title` (string, optional): Element title
- `value` (string, required): Value to set

**Example**:
```json
{"tool": "macax_set_value", "args": {
  "app_name": "TextEdit",
  "role": "textArea",
  "value": "Hello, World!"
}}
```

---

### macax_get_value

Get the current value of a UI element.

**Parameters**:
- `app_name` (string, required): Application name
- `identifier` (string, optional): Accessibility identifier
- `title` (string, optional): Element title

**Example**:
```json
{"tool": "macax_get_value", "args": {
  "app_name": "Calculator",
  "identifier": "display"
}}
```

**Returns**:
```
Value: 42
```

---

### macax_press_key

Simulate a key press.

**Parameters**:
- `key` (string, required): Key to press
- `modifiers` (array, optional): Modifier keys

**Supported modifiers**: `command`, `shift`, `option`, `control`

**Examples**:

```json
// Simple key press
{"tool": "macax_press_key", "args": {"key": "a"}}

// With modifiers (Cmd+S)
{"tool": "macax_press_key", "args": {
  "key": "s",
  "modifiers": ["command"]
}}

// Multiple modifiers (Cmd+Shift+N)
{"tool": "macax_press_key", "args": {
  "key": "n",
  "modifiers": ["command", "shift"]
}}

// Special keys
{"tool": "macax_press_key", "args": {"key": "return"}}
{"tool": "macax_press_key", "args": {"key": "escape"}}
{"tool": "macax_press_key", "args": {"key": "tab"}}
{"tool": "macax_press_key", "args": {"key": "delete"}}
```

**Special key names**:
- `return`, `enter`
- `escape`, `esc`
- `tab`
- `delete`, `backspace`
- `space`
- `up`, `down`, `left`, `right`
- `home`, `end`, `pageup`, `pagedown`
- `f1` through `f12`

## Common Roles

| Role | Description |
|------|-------------|
| `button` | Clickable button |
| `textField` | Single-line text input |
| `textArea` | Multi-line text input |
| `checkbox` | Checkbox control |
| `radioButton` | Radio button |
| `popUpButton` | Dropdown/popup menu |
| `slider` | Slider control |
| `table` | Table view |
| `list` | List view |
| `outline` | Outline/tree view |
| `group` | Container group |
| `window` | Application window |
| `sheet` | Modal sheet |
| `dialog` | Dialog window |
| `staticText` | Non-editable text |
| `image` | Image element |
| `scrollArea` | Scrollable container |
| `toolbar` | Toolbar |
| `menuBar` | Menu bar |
| `menu` | Menu |
| `menuItem` | Menu item |

## Best Practices

### 1. Use Accessibility Identifiers

When building apps you'll automate with G3, add accessibility identifiers:

**SwiftUI**:
```swift
Button("Submit") { ... }
    .accessibilityIdentifier("submit_button")
```

**UIKit**:
```swift
button.accessibilityIdentifier = "submit_button"
```

**AppKit**:
```swift
button.setAccessibilityIdentifier("submit_button")
```

Identifiers are more reliable than titles (which may be localized).

### 2. Inspect Before Automating

Always inspect the UI tree first:

```json
{"tool": "macax_get_ui_tree", "args": {"app_name": "MyApp", "max_depth": 4}}
```

This helps you understand:
- Element hierarchy
- Available identifiers
- Correct role names

### 3. Activate App First

Some actions require the app to be frontmost:

```json
{"tool": "macax_activate_app", "args": {"app_name": "MyApp"}}
{"tool": "macax_click", "args": {"app_name": "MyApp", "identifier": "button1"}}
```

### 4. Handle Timing

UI updates may take time. If an element isn't found:
1. Wait briefly
2. Retry the operation
3. Check if the app state changed

### 5. Prefer Identifiers Over Titles

```json
// Good: Uses identifier
{"tool": "macax_click", "args": {"app_name": "MyApp", "identifier": "save_btn"}}

// Less reliable: Uses title (may be localized)
{"tool": "macax_click", "args": {"app_name": "MyApp", "title": "Save"}}
```

## Example: Automating Calculator

```json
// 1. Activate Calculator
{"tool": "macax_activate_app", "args": {"app_name": "Calculator"}}

// 2. Inspect UI
{"tool": "macax_get_ui_tree", "args": {"app_name": "Calculator", "max_depth": 3}}

// 3. Click "5"
{"tool": "macax_click", "args": {"app_name": "Calculator", "title": "5"}}

// 4. Click "+"
{"tool": "macax_click", "args": {"app_name": "Calculator", "title": "+"}}

// 5. Click "3"
{"tool": "macax_click", "args": {"app_name": "Calculator", "title": "3"}}

// 6. Click "="
{"tool": "macax_click", "args": {"app_name": "Calculator", "title": "="}}

// 7. Read result
{"tool": "macax_get_value", "args": {"app_name": "Calculator", "role": "staticText"}}
```

## Troubleshooting

### "Accessibility permission denied"

1. Check System Preferences → Security & Privacy → Accessibility
2. Ensure your terminal app is listed and checked
3. Restart the terminal after granting permission

### "Application not found"

1. Use exact app name (case-sensitive)
2. Run `macax_list_apps` to see available apps
3. App must be running

### "Element not found"

1. Inspect UI tree to verify element exists
2. Check identifier/title spelling
3. Element may be in a different window or sheet
4. App state may have changed

### "Cannot perform action"

1. Element may be disabled
2. App may need to be frontmost
3. Element may not support the action
4. Check element role supports the operation

### Slow Performance

1. Reduce `max_depth` in `macax_get_ui_tree`
2. Use specific identifiers instead of searching
3. Complex apps have large UI trees

## Comparison with Other Tools

| Feature | macax | Vision Tools | WebDriver |
|---------|-------|--------------|----------|
| Native apps | ✅ | ✅ (via OCR) | ❌ |
| Web browsers | ✅ | ✅ | ✅ |
| Electron apps | ✅ | ✅ | Partial |
| Reliability | High | Medium | High |
| Setup | Permissions | None | Driver |
| Speed | Fast | Slower | Medium |

**Use macax when**:
- Automating native macOS apps
- You control the app and can add identifiers
- Need reliable, fast automation

**Use Vision tools when**:
- App doesn't expose accessibility
- Need to find text visually
- Cross-platform approach needed

**Use WebDriver when**:
- Automating web content
- Need JavaScript execution
- Testing web applications
