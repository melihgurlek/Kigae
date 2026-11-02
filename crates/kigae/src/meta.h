#pragma once
#include <QtGui/QGuiApplication>

// Whether Meta is down right now, from the compositor's modifier state (QML can't ask).
inline bool kigaeMetaHeld() { return QGuiApplication::queryKeyboardModifiers() & Qt::MetaModifier; }
