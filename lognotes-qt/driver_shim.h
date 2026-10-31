// Development-only helpers that let a test press real keys and look at the window.
// Compiled only when the `driver` feature is on, so the shipped app carries no test hooks.
//
// Free functions at global scope: cxx looks the symbols up unqualified, so a namespace here would need
// repeating in the bridge.

#pragma once

#include <QString>

class QQmlApplicationEngine;

// Remembers the engine's root object; every other call acts on it.
bool lognotes_attach(const QQmlApplicationEngine& engine);

// Types `text` through the window's focus item, as a keyboard would.
bool lognotes_type_text(const QString& text);

// Presses one key with Qt::KeyboardModifiers.
bool lognotes_press_key(int key, int modifiers);

// Writes the rendered window to `path` as PNG.
bool lognotes_save_png(const QString& path);

// Runs the event loop for `ms` so debounced saves and worker replies can land.
void lognotes_wait_ms(int ms);

// Object name (or class name) of whatever holds keyboard focus — where a keystroke would go.
QString lognotes_focus_name();

// How many rows the journal model currently has.
int lognotes_row_count();
